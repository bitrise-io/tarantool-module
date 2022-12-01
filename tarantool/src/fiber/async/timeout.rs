//! Allows a future to execute for a maximum amount of time.
//!
//! See [`Timeout`] documentation for more details.
//!
//! [`Timeout`]: struct@Timeout
use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;
use std::time::Instant;

use super::context::ContextExt;

/// Error returned by [`Timeout`]
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
#[error("deadline expired")]
pub struct Expired;

/// Future returned by [`timeout`](timeout).
pub struct Timeout<F> {
    future: F,
    deadline: Instant,
}

/// Requires a `Future` to complete before the specified duration has elapsed.
///
/// If the future completes before the duration has elapsed, then the completed
/// value is returned. Otherwise, an error is returned and the future is
/// canceled.
/// ```no_run
/// use tarantool::fiber::r#async::*;
/// use tarantool::fiber;
/// use std::time::Duration;
///
/// let (tx, rx) = oneshot::channel::<i32>();
///
/// // Wrap the future with a `Timeout` set to expire in 10 milliseconds.
/// if let Err(_) = fiber::block_on(timeout::timeout(Duration::from_millis(10), rx)) {
///     println!("did not receive value within 10 ms");
/// }      
/// ```
pub fn timeout<F: Future>(timeout: Duration, f: F) -> Timeout<F> {
    let now = Instant::now();
    let deadline = now.checked_add(timeout).unwrap_or_else(|| {
        // Add 30 years for now, because this is what tokio does:
        // https://github.com/tokio-rs/tokio/blob/22862739dddd49a94065aa7a917cde2dc8a3f6bc/tokio/src/time/instant.rs#L58-L62
        now + Duration::from_secs(60 * 60 * 24 * 365 * 30)
    });
    Timeout {
        future: f,
        deadline,
    }
}

impl<F: Future> Timeout<F> {
    #[inline]
    fn pin_get_future(self: Pin<&mut Self>) -> Pin<&mut F> {
        // This is okay because `field` is pinned when `self` is.
        unsafe { self.map_unchecked_mut(|s| &mut s.future) }
    }
}

impl<F: Future> Future for Timeout<F> {
    type Output = Result<F::Output, Expired>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let deadline = self.deadline;

        // First, try polling the future
        if let Poll::Ready(v) = self.pin_get_future().poll(cx) {
            Poll::Ready(Ok(v))
        } else if Instant::now() > deadline {
            Poll::Ready(Err(Expired)) // expired
        } else {
            // SAFETY: This is safe as long as the `Context` really
            // is the `ContextExt`. It's always true within provided
            // `block_on` async runtime.
            unsafe { ContextExt::set_deadline(cx, deadline) };
            Poll::Pending
        }
    }
}

#[cfg(feature = "tarantool_test")]
mod tests {
    use super::*;
    use crate::fiber;
    use crate::fiber::r#async::{oneshot, RecvError};
    use crate::test::{TestCase, TESTS};
    use crate::test_name;
    use linkme::distributed_slice;
    use std::time::Duration;

    const _0_SEC: Duration = Duration::ZERO;
    const _1_SEC: Duration = Duration::from_secs(1);

    #[distributed_slice(TESTS)]
    static INSTANT_FUTURE: TestCase = TestCase {
        name: test_name!("instant_future"),
        f: || {
            let fut = async { 78 };
            assert_eq!(fiber::block_on(fut), 78);

            let fut = timeout(Duration::ZERO, async { 79 });
            assert_eq!(fiber::block_on(fut), Ok(79));
        },
    };

    #[distributed_slice(TESTS)]
    static ACTUAL_TIMEOUT_PROMISE: TestCase = TestCase {
        name: test_name!("actual_timeout_promise"),
        f: || {
            let (tx, rx) = oneshot::channel::<i32>();
            let fut = async move { rx.timeout(_0_SEC).await };

            let jh = fiber::start(|| fiber::block_on(fut));
            assert_eq!(jh.join(), Err(Expired));
            drop(tx);
        },
    };

    #[distributed_slice(TESTS)]
    static DROP_TX_BEFORE_TIMEOUT: TestCase = TestCase {
        name: test_name!("drop_tx_before_timeout"),
        f: || {
            let (tx, rx) = oneshot::channel::<i32>();
            let fut = async move { rx.timeout(_1_SEC).await };

            let jh = fiber::start(move || fiber::block_on(fut));
            drop(tx);
            assert_eq!(jh.join(), Ok(Err(RecvError)));
        },
    };

    #[distributed_slice(TESTS)]
    static SEND_TX_BEFORE_TIMEOUT: TestCase = TestCase {
        name: test_name!("send_tx_before_timeout"),
        f: || {
            let (tx, rx) = oneshot::channel::<i32>();
            let fut = async move { rx.timeout(_1_SEC).await };

            let jh = fiber::start(move || fiber::block_on(fut));
            tx.send(400).unwrap();
            assert_eq!(jh.join(), Ok(Ok(400)));
        },
    };

    #[distributed_slice(TESTS)]
    static TIMEOUT_DURATION_MAX: TestCase = TestCase {
        name: test_name!("timeout_duration_max"),
        f: || {
            // must not panic
            timeout(Duration::MAX, async { 1 });
        },
    };
}