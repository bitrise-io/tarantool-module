//! The `net_box` module contains connector to remote Tarantool server instances via a network.
//!
//! You can call the following methods:
//! - [Conn::new()](struct.Conn.html#method.new) to connect and get a connection object (named `conn` for examples in this section),
//! - other `net_box` routines, to execute requests on the remote database system,
//! - [conn.close()](struct.Conn.html#method.close) to disconnect.
//!
//! All [Conn](struct.Conn.html) methods are fiber-safe, that is, it is safe to share and use the same connection object
//! across multiple concurrent fibers. In fact that is perhaps the best programming practice with Tarantool. When
//! multiple fibers use the same connection, all requests are pipelined through the same network socket, but each fiber
//! gets back a correct response. Reducing the number of active sockets lowers the overhead of system calls and increases
//! the overall server performance. However for some cases a single connection is not enough — for example, when it is
//! necessary to prioritize requests or to use different authentication IDs.
//!
//! Most [Conn](struct.Conn.html) methods allow a `options` argument. See [Options](struct.Options.html) structure docs
//! for details.
//!
//! The diagram below shows possible connection states and transitions:
//!
//! ![img](https://hb.bizmrg.com/tarantool-io/doc-builds/tarantool/2.6/images_en/net_states.svg?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=5qdnUajcfXmhe1ME4C5DqG%2F20201118%2Fru-msk%2Fs3%2Faws4_request&X-Amz-Date=20201118T130426Z&X-Amz-Expires=86400&X-Amz-SignedHeaders=host&X-Amz-Signature=d7df0b06513b11fa375875cfe6dc9de2dbc7073fe6ed1a11c8ce668b5fd02530)
//!
//! On this diagram:
//! - The state machine starts in the `initial` state.
//! - [Conn::new()](struct.Conn.html#method.new) method changes the state to `connecting` and spawns a worker fiber.
//! - If authentication and schema upload are required, it’s possible later on to re-enter the `fetch_schema` state
//! from `active` if a request fails due to a schema version mismatch error, so schema reload is triggered.
//! - [conn.close()](struct.Conn.html#method.close) method sets the state to `closed` and kills the worker. If the
//! transport is already in the `error` state, [close()](struct.Conn.html#method.close) does nothing.
//!
//! See also:
//! - [Lua reference: Module net.box](https://www.tarantool.io/en/doc/latest/reference/reference_lua/net_box/)

use std::io::{Cursor, Read, Write};
use std::net::{SocketAddr, ToSocketAddrs};

use bitflags::_core::cell::{Cell, RefCell};
use bitflags::_core::time::Duration;

pub use options::{ConnOptions, Options};

use crate::coio::CoIOStream;
use crate::error::Error;
use crate::tuple::{AsTuple, Tuple};

mod options;
mod protocol;

/// Connection to remote Tarantool server
#[derive(Default)]
pub struct Conn {
    addrs: Vec<SocketAddr>,
    options: ConnOptions,
    sync: Cell<u64>,
    session: RefCell<Option<Session>>,
}

struct Session {
    stream: CoIOStream,
    salt: Vec<u8>,
}

impl Conn {
    /// Create a new connection.
    ///
    /// The connection is established on demand, at the time of the first request. It can be re-established
    /// automatically after a disconnect (see [reconnect_after](struct.ConnOptions.html#structfield.reconnect_after) option).
    /// The returned conn object supports methods for making remote requests, such as select, update or delete.
    ///
    /// See also: [ConnOptions]()
    pub fn new(addr: &str, options: ConnOptions) -> Result<Self, Error> {
        Ok(Conn {
            options,
            addrs: addr.to_socket_addrs()?.collect(),
            sync: Cell::new(0),
            ..Default::default()
        })
    }

    /// Wait for connection to be active or closed.
    pub fn wait_connected(&self, timeout: Option<Duration>) -> Result<(), Error> {
        unimplemented!()
    }

    /// Show whether connection is active or closed.
    pub fn is_connected(&self) -> bool {
        unimplemented!()
    }

    /// Execute a PING command.
    ///
    /// - `options` – the supported option is `timeout`
    pub fn ping(&self, options: &Options) -> Result<(), Error> {
        let mut buf = Vec::new();
        let mut cur = Cursor::new(buf);

        let sync = self.next_sync();
        protocol::encode_ping(&mut cur, sync).unwrap();
        self.send_request(&cur.into_inner())?;
        // TBD
        Ok(())
    }

    /// Close a connection.
    pub fn close(self) {
        unimplemented!()
    }

    /// Call a remote stored procedure.
    ///
    /// `conn.call("func", &("1", "2", "3"))` is the remote-call equivalent of `func('1', '2', '3')`.
    /// That is, `conn.call` is a remote stored-procedure call.
    /// The return from `conn.call` is whatever the function returns.
    pub fn call<T>(
        &self,
        function_name: &str,
        args: &T,
        options: &Options,
    ) -> Result<Option<Tuple>, Error>
    where
        T: AsTuple,
    {
        let mut buf = Vec::new();
        let mut cur = Cursor::new(buf);

        let sync = self.next_sync();
        protocol::encode_call(&mut cur, sync, function_name, args).unwrap();
        // TBD
        Ok(None)
    }

    fn connect(&self) -> Result<(), Error> {
        let mut stream = CoIOStream::connect(&*self.addrs)?;
        let salt = protocol::decode_greeting(&mut stream)?;

        *self.session.borrow_mut() = Some(Session { stream, salt });

        Ok(())
    }

    fn send_request(&self, data: &Vec<u8>) -> Result<(), Error> {
        if self.session.borrow().is_none() {
            self.connect();
        }

        let mut session_ref_opt = self.session.borrow_mut();
        let session = session_ref_opt.as_mut().unwrap();
        session.stream.write_all(data);

        protocol::decode_response(&mut session.stream)?;

        Ok(())
    }

    fn next_sync(&self) -> u64 {
        let sync = self.sync.get();
        self.sync.set(sync + 1);
        sync
    }
}
