use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;

/// SDK session workers own handlers outside the HTTP connection futures. This barrier observes
/// their actual release so shutdown cannot return while they still own application files.
#[derive(Default)]
pub struct HandlerLifetime {
    active: AtomicUsize,
    released: Notify,
}
impl HandlerLifetime {
    pub fn acquire(self: &Arc<Self>) -> HandlerLease {
        self.active.fetch_add(1, Ordering::AcqRel);
        HandlerLease(self.clone())
    }
    pub async fn drained(&self) {
        loop {
            let released = self.released.notified();
            tokio::pin!(released);
            released.as_mut().enable();
            if self.active.load(Ordering::Acquire) == 0 {
                return;
            }
            released.await;
        }
    }
}
pub struct HandlerLease(Arc<HandlerLifetime>);
impl Clone for HandlerLease {
    fn clone(&self) -> Self {
        self.0.acquire()
    }
}
impl Drop for HandlerLease {
    fn drop(&mut self) {
        if self.0.active.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.released.notify_waiters();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::Future, task::Poll, time::Duration};

    #[tokio::test]
    async fn barrier_waits_for_every_clone_without_losing_the_release_signal() {
        tokio::time::timeout(Duration::from_secs(1), async {
            let lifetime = Arc::new(HandlerLifetime::default());
            lifetime.drained().await;
            let first = lifetime.acquire();
            let second = first.clone();
            let drained = lifetime.drained();
            tokio::pin!(drained);
            let pending =
                std::future::poll_fn(|cx| Poll::Ready(drained.as_mut().poll(cx).is_pending()));
            assert!(pending.await);
            drop(first);
            assert_eq!(lifetime.active.load(Ordering::Acquire), 1);
            drop(second);
            drained.await;
            assert_eq!(lifetime.active.load(Ordering::Acquire), 0);
        })
        .await
        .expect("owned handler release is bounded");
    }
}
