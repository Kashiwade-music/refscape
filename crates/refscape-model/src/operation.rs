use crate::{ErrorKind, JobId, ProjectEpoch, RefscapeError};
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: AtomicBool,
    waiters: Mutex<BTreeMap<u64, Waker>>,
    next: AtomicU64,
}

/// Runtime-independent cancellation. Registration and cancellation share a lock,
/// so a waiter cannot miss a cancellation between checking and subscribing.
#[derive(Debug, Default, Clone)]
pub struct CancellationToken(Arc<CancellationState>);
impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }
    pub fn cancel(&self) {
        let waiters = {
            let mut waiters = self.0.waiters.lock().unwrap_or_else(|e| e.into_inner());
            if self.0.cancelled.swap(true, Ordering::AcqRel) {
                return;
            }
            std::mem::take(&mut *waiters)
        };
        for waiter in waiters.into_values() {
            waiter.wake();
        }
    }
    pub fn cancelled(&self) -> Cancelled {
        Cancelled {
            token: self.clone(),
            registration: None,
        }
    }
}
pub struct Cancelled {
    token: CancellationToken,
    registration: Option<u64>,
}
impl Drop for Cancelled {
    fn drop(&mut self) {
        if let Some(id) = self.registration {
            self.token
                .0
                .waiters
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
        }
    }
}
impl Future for Cancelled {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let token = self.token.clone();
        let mut waiters = token.0.waiters.lock().unwrap_or_else(|e| e.into_inner());
        if token.is_cancelled() {
            if let Some(id) = self.registration.take() {
                waiters.remove(&id);
            }
            return Poll::Ready(());
        }
        let id = *self
            .registration
            .get_or_insert_with(|| token.0.next.fetch_add(1, Ordering::Relaxed));
        waiters.insert(id, cx.waker().clone());
        Poll::Pending
    }
}

#[derive(Debug, Clone)]
pub struct OperationContext {
    pub id: JobId,
    pub project: ProjectEpoch,
    pub deadline: Instant,
    pub cancel: CancellationToken,
}
impl OperationContext {
    pub fn new(id: JobId, project: ProjectEpoch, deadline: Instant) -> Self {
        Self {
            id,
            project,
            deadline,
            cancel: CancellationToken::new(),
        }
    }
    pub fn detached(timeout: Duration) -> Self {
        Self::new(
            JobId::default(),
            ProjectEpoch::default(),
            Instant::now() + timeout,
        )
    }
    pub fn check(&self) -> Result<(), RefscapeError> {
        if self.cancel.is_cancelled() {
            Err(
                RefscapeError::new(ErrorKind::Cancelled, "Operation cancelled")
                    .with_operation(self.id),
            )
        } else if Instant::now() >= self.deadline {
            Err(
                RefscapeError::new(ErrorKind::Timeout, "Operation timed out")
                    .with_operation(self.id),
            )
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::task::Wake;
    struct Notification(AtomicUsize);
    impl Wake for Notification {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    #[test]
    fn cancelled_waiters_wake_once_and_dropped_waiters_are_released() {
        let token = CancellationToken::new();
        let notification = Arc::new(Notification(AtomicUsize::new(0)));
        let waker = Waker::from(notification.clone());
        let mut context = Context::from_waker(&waker);
        {
            let mut cancelled = Box::pin(token.cancelled());
            assert!(cancelled.as_mut().poll(&mut context).is_pending());
            assert_eq!(token.0.waiters.lock().unwrap().len(), 1);
        }
        assert!(token.0.waiters.lock().unwrap().is_empty());
        let mut cancelled = Box::pin(token.cancelled());
        assert!(cancelled.as_mut().poll(&mut context).is_pending());
        token.cancel();
        token.cancel();
        assert_eq!(notification.0.load(Ordering::Relaxed), 1);
        assert!(cancelled.as_mut().poll(&mut context).is_ready());
        assert!(
            Box::pin(token.cancelled())
                .as_mut()
                .poll(&mut context)
                .is_ready()
        );
    }
    #[test]
    fn registration_cancel_race_never_loses_notification() {
        for _ in 0..100 {
            let token = CancellationToken::new();
            let notification = Arc::new(Notification(AtomicUsize::new(0)));
            let waker = Waker::from(notification.clone());
            let worker_token = token.clone();
            let worker = std::thread::spawn(move || worker_token.cancel());
            let mut cancelled = Box::pin(token.cancelled());
            let state = cancelled.as_mut().poll(&mut Context::from_waker(&waker));
            worker.join().unwrap();
            assert!(state.is_ready() || notification.0.load(Ordering::Relaxed) > 0);
            assert!(
                cancelled
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_ready()
            );
        }
    }
    #[test]
    fn absolute_deadlines_and_cancellation_have_distinct_errors() {
        let context = OperationContext::new(
            JobId(1),
            ProjectEpoch(2),
            Instant::now() - Duration::from_millis(1),
        );
        assert_eq!(context.check().unwrap_err().kind, ErrorKind::Timeout);
        context.cancel.cancel();
        assert_eq!(context.check().unwrap_err().kind, ErrorKind::Cancelled);
    }
}
