//! Tarantool based async [`Client`].
//!
//! Can be used only from inside Tarantool as it makes heavy use of fibers and coio.
//!
//! # Example
//! ```no_run
//! # async {
//! use tarantool::network::client::Client;
//!
//! let client = Client::connect("localhost", 3301).await.unwrap();
//! client.ping().await.unwrap();
//!
//! // Requests can also be easily combined with fiber::r#async::timeout
//! use tarantool::fiber::r#async::timeout::IntoTimeout as _;
//! use std::time::Duration;
//!
//! client.ping().timeout(Duration::from_secs(10)).await.unwrap();
//! # };
//! ```
//!
//! # Reusing Connection
//! Client can be cloned, and safely moved to a different fiber if needed, to reuse the same connection.
//! When multiple fibers use the same connection, all requests are pipelined through the same network socket, but each fiber
//! gets back a correct response. Reducing the number of active sockets lowers the overhead of system calls and increases
//! the overall server performance.
//!
//! # Implementation
//! Internally the client uses [`Protocol`] to get bytes that it needs to send
//! and push bytes that it gets from the network.
//!
//! On creation the client spawns sender and receiver worker threads. Which in turn
//! use coio based [`TcpStream`] as the transport layer.

pub mod tcp;

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Cursor, Error as IoError};
use std::rc::Rc;
use std::time::Duration;

use self::tcp::{Error as TcpError, TcpStream};

use super::protocol::api::{Call, Eval, Execute, Ping, Request};
use super::protocol::{self, Error as ProtocolError, Protocol, SizeHint, SyncIndex};
use crate::fiber;
use crate::fiber::r#async::IntoOnDrop as _;
use crate::fiber::r#async::{oneshot, watch};
use crate::tuple::{ToTupleBuffer, Tuple};

use futures::io::{ReadHalf, WriteHalf};
use futures::{AsyncReadExt, AsyncWriteExt};

/// Error returned by [`Client`].
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("tcp stream error: {0}")]
    Tcp(#[from] TcpError),
    #[error("io error: {0}")]
    Io(#[from] IoError),
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("closed with error: {0}")]
    ClosedWithErr(String),
    #[error("{0}")]
    Other(String),
}

#[derive(Clone, Debug)]
enum State {
    Alive,
    ClosedManually,
    ClosedWithError(String),
}

impl State {
    fn is_alive(&self) -> bool {
        matches!(self, Self::Alive)
    }

    fn is_closed(&self) -> bool {
        !self.is_alive()
    }
}

type WorkerHandle = fiber::JoinHandle<'static, ()>;

#[derive(Debug)]
struct ClientInner {
    protocol: Protocol,
    awaiting_response: HashMap<SyncIndex, oneshot::Sender<Result<(), Error>>>,
    state: State,
    close_token: Option<tcp::CloseToken>,
    worker_handles: Vec<WorkerHandle>,
    sender_waker: watch::Sender<()>,
}

impl ClientInner {
    pub fn new(config: protocol::Config, sender_waker: watch::Sender<()>) -> Self {
        Self {
            protocol: Protocol::with_config(config),
            awaiting_response: HashMap::new(),
            state: State::Alive,
            close_token: None,
            worker_handles: Vec::new(),
            sender_waker,
        }
    }
}

/// Wakes sender if `protocol` has new outgoing data.
///
/// # Errors
/// Returns an error if `sender_waker` channel receivers are holding a reference to the previous value.
/// Which generally shouldn't be the case as it is an empty value.
fn wake_sender(client: &RefCell<ClientInner>) -> Result<(), watch::SendError<()>> {
    let len = client.borrow().protocol.ready_outgoing_len();
    if len > 0 {
        client.borrow().sender_waker.send(())?;
    }
    Ok(())
}

/// Actual client that can be used to send and receive messages to tarantool instance.
///
/// Can be cloned and moved into different fibers for connection to be reused.
///
/// See [`super::client`] for examples.
// WARNING: Attention should be payed not to borrow inner client across await and yield points.
#[derive(Clone, Debug)]
pub struct Client(Rc<RefCell<ClientInner>>);

impl Client {
    /// Creates a new client and tries to establish connection
    /// to `url:port`
    ///
    /// # Errors
    /// Error is returned if an attempt to connect failed.
    /// See [`Error`].
    pub async fn connect(url: &str, port: u16) -> Result<Self, Error> {
        Self::connect_with_config(url, port, Default::default()).await
    }

    /// Creates a new client and tries to establish connection
    /// to `url:port`
    ///
    /// Takes explicit `config` in comparison to [`Client::connect`]
    /// where default values are used.
    ///
    /// # Errors
    /// Error is returned if an attempt to connect failed.
    /// See [`Error`].
    pub async fn connect_with_config(
        url: &str,
        port: u16,
        config: protocol::Config,
    ) -> Result<Self, Error> {
        let (sender_waker_tx, sender_waker_rx) = watch::channel(());
        let mut client = ClientInner::new(config, sender_waker_tx);
        let stream = TcpStream::connect(url, port).await?;
        client.close_token = Some(stream.close_token());

        let (reader, writer) = stream.split();
        let client = Rc::new(RefCell::new(client));

        // start receiver in a separate fiber
        let receiver_handle = fiber::Builder::new()
            .func_async(receiver(client.clone(), reader))
            .name("network-client-receiver")
            .start()
            .unwrap();

        // start sender in a separate fiber
        let sender_handle = fiber::Builder::new()
            .func_async(sender(client.clone(), writer, sender_waker_rx))
            .name("network-client-sender")
            .start()
            .unwrap();
        client.borrow_mut().worker_handles = vec![receiver_handle, sender_handle];
        Ok(Self(client))
    }

    fn check_state(&self) -> Result<(), Error> {
        match self.0.borrow().state.clone() {
            State::Alive => Ok(()),
            State::ClosedManually => unreachable!("All client handles are dropped at this point"),
            State::ClosedWithError(err) => Err(Error::ClosedWithErr(err)),
        }
    }

    /// Send [`Request`] and wait for response.
    /// This function yields.
    ///
    /// # Errors
    /// In case of `ClosedWithErr` it is suggested to recreate the connection.
    /// Other errors are self-descriptive.
    async fn send<R: Request>(&self, request: &R) -> Result<R::Response, Error> {
        self.check_state()?;
        let sync = self.0.borrow_mut().protocol.send_request(request)?;
        let (tx, rx) = oneshot::channel();
        self.0.borrow_mut().awaiting_response.insert(sync, tx);
        wake_sender(&self.0).unwrap();
        // Cleanup `awaiting_response` entry in case of `send` future cancelation
        // at this `.await`.
        // `send` can be canceled for example with `Timeout`.
        rx.on_drop(|| {
            let _ = self.0.borrow_mut().awaiting_response.remove(&sync);
        })
        .await
        .expect("Channel should be open")?;
        Ok(self
            .0
            .borrow_mut()
            .protocol
            .take_response(sync, request)
            .expect("Is present at this point")?)
    }

    /// Execute a PING command.
    pub async fn ping(&self) -> Result<(), Error> {
        self.send(&Ping).await
    }

    /// Call a remote stored procedure.
    ///
    /// `conn.call("func", &("1", "2", "3"))` is the remote-call equivalent of `func('1', '2', '3')`.
    /// That is, `conn.call` is a remote stored-procedure call.
    /// The return from `conn.call` is whatever the function returns.
    pub async fn call<T: ToTupleBuffer>(
        &self,
        fn_name: &str,
        args: &T,
    ) -> Result<Option<Tuple>, Error> {
        self.send(&Call { fn_name, args }).await
    }

    /// Evaluates and executes the expression in Lua-string, which may be any statement or series of statements.
    ///
    /// An execute privilege is required; if the user does not have it, an administrator may grant it with
    /// `box.schema.user.grant(username, 'execute', 'universe')`.
    ///
    /// To ensure that the return from `eval` is whatever the Lua expression returns, begin the Lua-string with the
    /// word `return`.
    pub async fn eval<T: ToTupleBuffer>(
        &self,
        expr: &str,
        args: &T,
    ) -> Result<Option<Tuple>, Error> {
        self.send(&Eval { args, expr }).await
    }

    /// Execute sql query remotely.
    pub async fn execute<T: ToTupleBuffer>(
        &self,
        sql: &str,
        bind_params: &T,
        limit: Option<usize>,
    ) -> Result<Vec<Tuple>, Error> {
        self.send(&Execute {
            sql,
            bind_params,
            limit,
        })
        .await
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // 3 means this client and 2 fibers: receiver and sender
        if Rc::strong_count(&self.0) <= 3 {
            let mut client = self.0.borrow_mut();
            // Stop fibers
            client.state = State::ClosedManually;

            let close_token = client.close_token.take();
            let handles: Vec<_> = client.worker_handles.drain(..).collect();
            // Wake sender so it can exit loop
            client.sender_waker.send(()).unwrap();

            // Drop ref before executing code that switches fibers.
            drop(client);
            if let Some(close_token) = close_token {
                // Close TCP stream to wake fibers waiting on coio events
                let _ = close_token.close();
            }
            // Join fibers
            for handle in handles {
                handle.join();
            }
        }
    }
}

macro_rules! handle_result {
    ($client:expr, $e:expr) => {
        match $e {
            Ok(value) => value,
            Err(err) => {
                let err: Error = err.into();
                let str_err = err.to_string();
                $client.state = State::ClosedWithError(err.to_string());
                // Notify all subscribers on closing
                let subscriptions: HashMap<_, _> = $client.awaiting_response.drain().collect();
                for (_, subscription) in subscriptions {
                    // We don't care about errors at this point
                    let _ = subscription.send(Err(Error::ClosedWithErr(str_err.clone())));
                }
                return;
            }
        }
    };
}

/// Sender work loop. Yields on each iteration and during awaits.
async fn sender(
    client: Rc<RefCell<ClientInner>>,
    mut writer: WriteHalf<TcpStream>,
    mut waker: watch::Receiver<()>,
) {
    loop {
        if client.borrow().state.is_closed() {
            return;
        }
        // TODO: Set max drain
        let data: Vec<_> = client
            .borrow_mut()
            .protocol
            .drain_outgoing_data(None)
            .collect();
        if data.is_empty() {
            // Wait for explicit wakeup, it should happen when there is new outgoing data
            waker.changed().await.expect("channel should be open");
        } else {
            let result = writer.write_all(&data).await;
            handle_result!(client.borrow_mut(), result);
        }
    }
}

/// Receiver work loop. Yields on each iteration and during awaits.
async fn receiver(client: Rc<RefCell<ClientInner>>, mut reader: ReadHalf<TcpStream>) {
    let mut hint = client.borrow().protocol.read_size_hint();
    loop {
        if client.borrow().state.is_closed() {
            return;
        }
        match hint {
            SizeHint::Hint(size) => {
                let mut buf = vec![0; size];
                handle_result!(client.borrow_mut(), reader.read_exact(&mut buf).await);
                let result = client
                    .borrow_mut()
                    .protocol
                    .process_incoming(&mut Cursor::new(buf));
                hint = client.borrow().protocol.read_size_hint();
                let result = handle_result!(client.borrow_mut(), result);
                if let Some(sync) = result {
                    let subscription = client.borrow_mut().awaiting_response.remove(&sync);
                    if let Some(subscription) = subscription {
                        subscription
                            .send(Ok(()))
                            .expect("cannot be closed at this point");
                    } else {
                        log::warn!("received unwaited message for {sync:?}");
                    }
                }
                wake_sender(&client).unwrap();
            }
            SizeHint::FirstU32 => {
                // Read 5 bytes, 1st is a marker
                let mut buf = vec![0; 5];
                handle_result!(client.borrow_mut(), reader.read_exact(&mut buf).await);
                let result = rmp::decode::read_u32(&mut Cursor::new(buf));
                let mut client_ref = client.borrow_mut();
                let new_hint = handle_result!(client_ref, result.map_err(ProtocolError::from));
                if new_hint > 0 {
                    hint = SizeHint::Hint(new_hint as usize)
                } else {
                    handle_result!(
                        client_ref,
                        Err(Error::Other("unexpected zero message length".to_owned()))
                    )
                }
            }
        }
    }
}

#[cfg(feature = "internal_test")]
mod tests {
    use super::*;
    use crate::fiber::r#async::timeout::IntoTimeout as _;
    use crate::space::Space;
    use crate::test::util::TARANTOOL_LISTEN;

    async fn test_client() -> Client {
        Client::connect_with_config(
            "localhost",
            TARANTOOL_LISTEN,
            protocol::Config {
                creds: Some(("test_user".to_owned(), "password".to_owned())),
            },
        )
        .timeout(Duration::from_secs(3))
        .await
        .unwrap()
    }

    #[crate::test(tarantool = "crate")]
    fn connect() {
        fiber::block_on(async {
            let _client = Client::connect("localhost", TARANTOOL_LISTEN)
                .await
                .unwrap();
        });
    }

    #[crate::test(tarantool = "crate")]
    fn connect_failure() {
        fiber::block_on(async {
            // Can be any other unused port
            let err = Client::connect("localhost", 3300).await.unwrap_err();
            assert!(matches!(dbg!(err), Error::Tcp(_)))
        });
    }

    #[crate::test(tarantool = "crate")]
    fn ping() {
        fiber::block_on(async {
            let client = test_client().await;

            for _ in 0..5 {
                client.ping().timeout(Duration::from_secs(3)).await.unwrap();
            }
        });
    }

    #[crate::test(tarantool = "crate")]
    fn ping_concurrent() {
        let client = fiber::block_on(test_client());
        let fiber_a = fiber::start_async(async {
            client.ping().timeout(Duration::from_secs(3)).await.unwrap()
        });
        let fiber_b = fiber::start_async(async {
            client.ping().timeout(Duration::from_secs(3)).await.unwrap()
        });
        fiber_a.join();
        fiber_b.join();
    }

    #[crate::test(tarantool = "crate")]
    fn execute() {
        Space::find("test_s1")
            .unwrap()
            .insert(&(6001, "6001"))
            .unwrap();
        Space::find("test_s1")
            .unwrap()
            .insert(&(6002, "6002"))
            .unwrap();

        fiber::block_on(async {
            let client = test_client().await;

            let result = client
                .execute(r#"SELECT * FROM "test_s1""#, &(), None)
                .timeout(Duration::from_secs(3))
                .await
                .unwrap();
            assert!(result.len() >= 2);

            let result = client
                .execute(r#"SELECT * FROM "test_s1" WHERE "id" = ?"#, &(6002,), None)
                .timeout(Duration::from_secs(3))
                .await
                .unwrap();

            assert_eq!(result.len(), 1);
            assert_eq!(
                result.get(0).unwrap().decode::<(u64, String)>().unwrap(),
                (6002, "6002".to_string())
            );
        });
    }

    #[crate::test(tarantool = "crate")]
    fn call() {
        fiber::block_on(async {
            let client = test_client().await;

            let result = client
                .call("test_stored_proc", &(1, 2))
                .timeout(Duration::from_secs(3))
                .await
                .unwrap();
            assert_eq!(result.unwrap().decode::<(i32,)>().unwrap(), (3,));
        });
    }

    #[crate::test(tarantool = "crate")]
    fn invalid_call() {
        fiber::block_on(async {
            let client = test_client().await;

            let err = client
                .call("unexistent_proc", &())
                .timeout(Duration::from_secs(3))
                .await
                .unwrap_err()
                .to_string();
            assert_eq!(err, "protocol error: service responded with error: Procedure 'unexistent_proc' is not defined");
        });
    }

    #[crate::test(tarantool = "crate")]
    fn eval() {
        fiber::block_on(async {
            let client = test_client().await;

            let result = client
                .eval("return ...", &(1, 2))
                .timeout(Duration::from_secs(3))
                .await
                .unwrap();
            assert_eq!(result.unwrap().decode::<(i32, i32)>().unwrap(), (1, 2));
        });
    }
}
