use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

pub use futures_core::{Future, Stream};
pub use futures_io::{AsyncRead, AsyncSeek, AsyncWrite};

#[macro_export]
macro_rules! ready {
    ($e:expr $(,)?) => {
        match $e {
            std::task::Poll::Ready(t) => t,
            std::task::Poll::Pending => return std::task::Poll::Pending,
        }
    };
}

#[macro_export]
macro_rules! pin {
    ($($x:ident),* $(,)?) => {
        $(
            let mut $x = $x;

            #[allow(unused_mut)]
            let mut $x = unsafe {
                std::pin::Pin::new_unchecked(&mut $x)
            };
        )*
    }
}

pub mod future {
    use super::*;

    /// Future for the [`poll_fn`] function.
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    pub struct PollFn<F> {
        f: F,
    }

    impl<F> Unpin for PollFn<F> {}

    /// Creates a new future wrapping around a function returning [`Poll`].
    ///
    /// Polling the returned future delegates to the wrapped function.
    pub fn poll_fn<T, F>(f: F) -> PollFn<F>
    where
        F: FnMut(&mut Context<'_>) -> Poll<T>,
    {
        PollFn { f }
    }

    impl<F> fmt::Debug for PollFn<F> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("PollFn").finish()
        }
    }

    impl<T, F> Future for PollFn<F>
    where
        F: FnMut(&mut Context<'_>) -> Poll<T>,
    {
        type Output = T;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
            (&mut self.f)(cx)
        }
    }
}

pub mod stream {
    use super::*;

    /// Creates a `Stream` from a seed and a closure returning a `Future`.
    ///
    /// This function is the dual for the `Stream::fold()` adapter: while
    /// `Stream::fold()` reduces a `Stream` to one single value, `unfold()` creates a
    /// `Stream` from a seed value.
    ///
    /// `unfold()` will call the provided closure with the provided seed, then wait
    /// for the returned `Future` to complete with `(a, b)`. It will then yield the
    /// value `a`, and use `b` as the next internal state.
    ///
    /// If the closure returns `None` instead of `Some(Future)`, then the `unfold()`
    /// will stop producing items and return `Poll::Ready(None)` in future
    /// calls to `poll()`.
    ///
    /// This function can typically be used when wanting to go from the "world of
    /// futures" to the "world of streams": the provided closure can build a
    /// `Future` using other library functions working on futures, and `unfold()`
    /// will turn it into a `Stream` by repeating the operation.
    pub fn unfold<T, F, Fut, Item>(init: T, f: F) -> Unfold<T, F, Fut>
    where
        F: FnMut(T) -> Fut,
        Fut: Future<Output = Option<(Item, T)>>,
    {
        Unfold {
            f,
            state: Some(init),
            fut: None,
        }
    }

    /// Stream for the [`unfold`] function.
    #[must_use = "streams do nothing unless polled"]
    pub struct Unfold<T, F, Fut> {
        f: F,
        state: Option<T>,
        fut: Option<Fut>,
    }

    impl<T, F, Fut> fmt::Debug for Unfold<T, F, Fut>
    where
        T: fmt::Debug,
        Fut: fmt::Debug,
    {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Unfold")
                .field("state", &self.state)
                .field("fut", &self.fut)
                .finish()
        }
    }

    impl<T, F, Fut, Item> Stream for Unfold<T, F, Fut>
    where
        F: FnMut(T) -> Fut,
        Fut: Future<Output = Option<(Item, T)>>,
    {
        type Item = Item;

        fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let mut this = unsafe { self.get_unchecked_mut() };

            if let Some(state) = this.state.take() {
                this.fut = Some((this.f)(state));
            }

            let fut = unsafe {
                Pin::new_unchecked(
                    this.fut
                        .as_mut()
                        .expect("Unfold must not be polled after it returned `Poll::Ready(None)`"),
                )
            };
            let step = futures_core::ready!(fut.poll(cx));
            this.fut = None;

            if let Some((item, next_state)) = step {
                this.state = Some(next_state);
                Poll::Ready(Some(item))
            } else {
                Poll::Ready(None)
            }
        }
    }
}