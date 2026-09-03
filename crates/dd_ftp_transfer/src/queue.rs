use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::Utc;
use dd_ftp_core::{TransferJob, TransferStatus};
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct TransferQueue {
    pub pending: Vec<TransferJob>,
    pub active: Vec<TransferJob>,
    pub completed: Vec<TransferJob>,
    pub failed: Vec<TransferJob>,
    pub cancelled: Vec<TransferJob>,
    last_progress: HashMap<Uuid, (Instant, u64)>,
    last_speed: HashMap<Uuid, f64>,
}

impl TransferQueue {
    pub fn enqueue(&mut self, mut job: TransferJob) {
        job.status = TransferStatus::Pending;
        self.pending.push(job);
    }

    pub fn start_next(&mut self) -> Option<TransferJob> {
        if self.pending.is_empty() {
            return None;
        }

        let mut job = self.pending.remove(0);
        job.status = TransferStatus::Active;
        self.active.push(job.clone());
        Some(job)
    }

    fn clear_progress(&mut self, id: Uuid) {
        self.last_progress.remove(&id);
        self.last_speed.remove(&id);
    }

    pub fn mark_completed(&mut self, mut job: TransferJob) {
        job.status = TransferStatus::Completed;
        self.active.retain(|j| j.id != job.id);
        self.clear_progress(job.id);
        self.completed.push(job);
    }

    pub fn mark_failed(&mut self, mut job: TransferJob) {
        job.status = TransferStatus::Failed;
        self.active.retain(|j| j.id != job.id);
        self.clear_progress(job.id);
        self.failed.push(job);
    }

    pub fn mark_cancelled(&mut self, mut job: TransferJob) {
        job.status = TransferStatus::Cancelled;
        self.active.retain(|j| j.id != job.id);
        self.clear_progress(job.id);
        self.cancelled.push(job);
    }

    pub fn retry_last_failed(&mut self) -> Option<TransferJob> {
        let mut job = self.failed.pop()?;
        job.retries = job.retries.saturating_add(1);
        job.status = TransferStatus::Pending;
        job.last_error = None;
        self.pending.push(job.clone());
        Some(job)
    }

    pub fn update_active_progress(&mut self, job_id: Uuid, transferred: u64, size: Option<u64>) {
        self.update_active_progress_at(job_id, transferred, size, Instant::now());
    }

    pub fn update_active_progress_at(
        &mut self,
        job_id: Uuid,
        transferred: u64,
        size: Option<u64>,
        now: Instant,
    ) {
        {
            let Some(job) = self.active.iter_mut().find(|j| j.id == job_id) else {
                return;
            };
            job.transferred_bytes = transferred;
            if size.is_some() {
                job.size_bytes = size;
            }
            job.updated_at = Utc::now();
        }
        if let Some((t0, bytes0)) = self.last_progress.get(&job_id).copied() {
            let dt = now.saturating_duration_since(t0);
            if dt >= Duration::from_millis(100) {
                let delta = transferred.saturating_sub(bytes0);
                let speed = delta as f64 / dt.as_secs_f64();
                self.last_speed.insert(job_id, speed);
                self.last_progress.insert(job_id, (now, transferred));
            }
        } else {
            self.last_progress.insert(job_id, (now, transferred));
        }
    }

    /// Bytes/sec and remaining time. ETA is `None` when size or speed is missing.
    pub fn speed_and_eta(&self, job_id: Uuid) -> Option<(f64, Option<Duration>)> {
        let speed = *self.last_speed.get(&job_id)?;
        let job = self.active.iter().find(|j| j.id == job_id)?;
        let eta = job.size_bytes.and_then(|size| {
            if speed > 0.0 {
                let remain = size.saturating_sub(job.transferred_bytes) as f64 / speed;
                Some(Duration::from_secs_f64(remain.max(0.0)))
            } else {
                None
            }
        });
        Some((speed, eta))
    }

    pub fn clear_pending(&mut self) -> usize {
        let count = self.pending.len();
        self.pending.clear();
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dd_ftp_core::{TransferDirection, TransferJob, TransferStatus};
    use uuid::Uuid;

    fn job(name: &str) -> TransferJob {
        TransferJob::new(
            format!("/tmp/{name}"),
            format!("/pub/{name}"),
            TransferDirection::Upload,
        )
    }

    #[test]
    fn enqueue_adds_pending() {
        let mut q = TransferQueue::default();
        q.enqueue(job("a"));
        q.enqueue(job("b"));
        assert_eq!(q.pending.len(), 2);
        assert_eq!(q.pending[0].status, TransferStatus::Pending);
        assert_eq!(q.pending[1].remote_path, "/pub/b");
    }

    #[test]
    fn start_next_is_fifo() {
        let mut q = TransferQueue::default();
        q.enqueue(job("first"));
        q.enqueue(job("second"));
        let a = q.start_next().expect("first");
        assert_eq!(a.remote_path, "/pub/first");
        assert_eq!(a.status, TransferStatus::Active);
        assert_eq!(q.pending.len(), 1);
        assert_eq!(q.active.len(), 1);
        let b = q.start_next().expect("second");
        assert_eq!(b.remote_path, "/pub/second");
        assert!(q.pending.is_empty());
        assert_eq!(q.active.len(), 2);
    }

    #[test]
    fn mark_completed_failed_cancelled() {
        let mut q = TransferQueue::default();
        q.enqueue(job("ok"));
        q.enqueue(job("fail"));
        q.enqueue(job("cancel"));
        let ok = q.start_next().unwrap();
        let fail = q.start_next().unwrap();
        let cancel = q.start_next().unwrap();
        q.mark_completed(ok);
        q.mark_failed(fail);
        q.mark_cancelled(cancel);
        assert!(q.active.is_empty());
        assert_eq!(q.completed.len(), 1);
        assert_eq!(q.completed[0].status, TransferStatus::Completed);
        assert_eq!(q.failed.len(), 1);
        assert_eq!(q.failed[0].status, TransferStatus::Failed);
        assert_eq!(q.cancelled.len(), 1);
        assert_eq!(q.cancelled[0].status, TransferStatus::Cancelled);
    }

    #[test]
    fn retry_last_failed_increments_retries() {
        let mut q = TransferQueue::default();
        let mut j = job("a");
        j.retries = 0;
        q.mark_failed(j);
        let retried = q.retry_last_failed().expect("retry");
        assert_eq!(retried.retries, 1);
        assert_eq!(retried.status, TransferStatus::Pending);
        assert_eq!(q.pending.len(), 1);
        assert!(q.failed.is_empty());
    }

    #[test]
    fn clear_pending_returns_count() {
        let mut q = TransferQueue::default();
        q.enqueue(job("a"));
        q.enqueue(job("b"));
        q.enqueue(job("c"));
        assert_eq!(q.clear_pending(), 3);
        assert!(q.pending.is_empty());
        assert_eq!(q.clear_pending(), 0);
    }

    #[test]
    fn update_active_progress_ignores_unknown_ids() {
        let mut q = TransferQueue::default();
        q.enqueue(job("a"));
        let started = q.start_next().unwrap();
        q.update_active_progress(Uuid::nil(), 99, Some(100));
        assert_eq!(q.active[0].transferred_bytes, 0);
        q.update_active_progress(started.id, 10, Some(20));
        assert_eq!(q.active[0].transferred_bytes, 10);
        assert_eq!(q.active[0].size_bytes, Some(20));
    }

    #[test]
    fn cancel_does_not_drop_pending() {
        let mut q = TransferQueue::default();
        q.enqueue(job("active"));
        q.enqueue(job("keep"));
        let active = q.start_next().unwrap();
        assert_eq!(active.remote_path, "/pub/active");
        assert_eq!(q.pending.len(), 1);
        q.mark_cancelled(active);
        assert_eq!(q.pending.len(), 1);
        assert_eq!(q.pending[0].remote_path, "/pub/keep");
        assert_eq!(q.cancelled.len(), 1);
    }

    #[test]
    fn two_progress_samples_100ms_apart_yield_speed() {
        use std::time::Duration;
        let mut q = TransferQueue::default();
        q.enqueue(job("a"));
        let started = q.start_next().unwrap();
        let t0 = Instant::now();
        q.update_active_progress_at(started.id, 0, Some(10_000), t0);
        assert!(q.speed_and_eta(started.id).is_none());
        q.update_active_progress_at(
            started.id,
            1_000,
            Some(10_000),
            t0 + Duration::from_millis(100),
        );
        let (speed, eta) = q.speed_and_eta(started.id).expect("speed");
        assert!(speed > 0.0, "speed was {speed}");
        assert!(eta.is_some());
    }

    #[test]
    fn missing_size_eta_is_none() {
        use std::time::Duration;
        let mut q = TransferQueue::default();
        q.enqueue(job("a"));
        let started = q.start_next().unwrap();
        let t0 = Instant::now();
        q.update_active_progress_at(started.id, 0, None, t0);
        q.update_active_progress_at(started.id, 1_000, None, t0 + Duration::from_millis(150));
        let (speed, eta) = q.speed_and_eta(started.id).expect("speed");
        assert!(speed > 0.0);
        assert!(eta.is_none(), "missing size must not produce an ETA");
    }
}
