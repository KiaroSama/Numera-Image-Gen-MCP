use super::finalization::{SharedStore, locked};
use crate::jobs::{Receipt, Store};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

/// Generation-fenced renewal belongs to the operation; stopping it waits for the owned task.
pub struct Lease {
    stop: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
    receipt: Arc<Mutex<Receipt>>,
}
impl Lease {
    pub fn start(
        store: SharedStore,
        receipt: Receipt,
        cancel: CancellationToken,
        observe_cancel: bool,
    ) -> Self {
        let stop = CancellationToken::new();
        let done = stop.clone();
        let receipt = Arc::new(Mutex::new(receipt));
        let current = receipt.clone();
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(if observe_cancel {
                200
            } else {
                Store::LEASE_MS as u64 / 3
            }));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut renewed = tokio::time::Instant::now();
            loop {
                tokio::select! {
                    _=done.cancelled()=>break,
                    _=interval.tick()=>{
                        let result=(|| {
                            let receipt=current.lock().ok()?;
                            let id=receipt.id().ok()?;
                            let store=locked(&store).ok()?;
                            if observe_cancel && store.cancelled(id).ok()? {cancel.cancel();}
                            if renewed.elapsed()>=Duration::from_millis(Store::LEASE_MS as u64/3) {
                                if !store.renew(id,Some(&receipt)).ok()? {return None;}
                                renewed=tokio::time::Instant::now();
                            }
                            Some(())
                        })();
                        if result.is_none(){cancel.cancel();break;}
                    }
                }
            }
        });
        Self {
            stop,
            task: Some(task),
            receipt,
        }
    }
    pub fn update(&self, receipt: &Receipt) {
        if let Ok(mut current) = self.receipt.lock() {
            *current = receipt.clone();
        }
    }
    pub async fn close(mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
