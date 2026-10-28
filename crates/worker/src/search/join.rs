//! The two concurrency shapes search needs, on one thread: run futures with at most `n` in flight, and
//! race a future against a deadline. Futures are lazy, so a future not yet polled has not started.

use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::Poll;

/// Every future's output, in input order, with at most `n` polled at once.
pub async fn bounded<F: Future>(futs: Vec<F>, n: usize) -> Vec<F::Output> {
    let mut slots: Vec<Option<Pin<Box<F>>>> = futs.into_iter().map(|f| Some(Box::pin(f))).collect();
    let mut out: Vec<Option<F::Output>> = slots.iter().map(|_| None).collect();
    poll_fn(|cx| {
        let mut active = 0;
        for (i, slot) in slots.iter_mut().enumerate() {
            if active >= n.max(1) {
                break;
            }
            let Some(f) = slot else { continue };
            active += 1;
            if let Poll::Ready(v) = f.as_mut().poll(cx) {
                out[i] = Some(v);
                *slot = None;
                active -= 1;
            }
        }
        if slots.iter().all(Option::is_none) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
    out.into_iter()
        .map(|o| o.expect("every slot finished"))
        .collect()
}

/// Both outputs, the two futures polled together.
pub async fn both<A: Future, B: Future>(a: A, b: B) -> (A::Output, B::Output) {
    let mut a = Box::pin(a);
    let mut b = Box::pin(b);
    let mut ra = None;
    let mut rb = None;
    poll_fn(|cx| {
        if ra.is_none()
            && let Poll::Ready(v) = a.as_mut().poll(cx)
        {
            ra = Some(v);
        }
        if rb.is_none()
            && let Poll::Ready(v) = b.as_mut().poll(cx)
        {
            rb = Some(v);
        }
        if ra.is_some() && rb.is_some() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
    (ra.expect("a finished"), rb.expect("b finished"))
}

/// `Some(output)` when `work` finishes before `deadline`; `None` otherwise (its result is dropped).
/// `work` is polled first, so work that is already done wins a deadline that is also done.
pub async fn before<F: Future, D: Future<Output = ()>>(work: F, deadline: D) -> Option<F::Output> {
    let mut work = Box::pin(work);
    let mut deadline = Box::pin(deadline);
    poll_fn(|cx| {
        if let Poll::Ready(v) = work.as_mut().poll(cx) {
            return Poll::Ready(Some(v));
        }
        if deadline.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block<T>(f: impl Future<Output = T>) -> T {
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        let mut f = Box::pin(f);
        loop {
            if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    /// Ready after `n` polls.
    fn after_polls(n: u32, v: u32) -> impl Future<Output = u32> {
        let mut left = n;
        poll_fn(move |cx| {
            if left == 0 {
                Poll::Ready(v)
            } else {
                left -= 1;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
    }

    #[test]
    fn bounded_keeps_order_and_deadline_races() {
        let futs = vec![after_polls(3, 1), after_polls(0, 2), after_polls(1, 3)];
        assert_eq!(block(bounded(futs, 2)), vec![1, 2, 3]);
        assert_eq!(block(before(after_polls(0, 7), async {})), Some(7));
        assert_eq!(block(before(after_polls(5, 7), async {})), None);
        assert_eq!(block(both(after_polls(2, 1), after_polls(0, 2))), (1, 2));
    }
}
