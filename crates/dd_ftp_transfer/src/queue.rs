use dd_ftp_core::{TransferJob, TransferStatus};
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct TransferQueue {
    pub pending: Vec<TransferJob>,
    pub active: Vec<TransferJob>,
    pub completed: Vec<TransferJob>,
    pub failed: Vec<TransferJob>,
    pub cancelled: Vec<TransferJob>,
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

    pub fn mark_completed(&mut self, mut job: TransferJob) {
        job.status = TransferStatus::Completed;
        self.active.retain(|j| j.id != job.id);
        self.completed.push(job);
    }

    pub fn mark_failed(&mut self, mut job: TransferJob) {
        job.status = TransferStatus::Failed;
        self.active.retain(|j| j.id != job.id);
        self.failed.push(job);
    }

    pub fn mark_cancelled(&mut self, mut job: TransferJob) {
        job.status = TransferStatus::Cancelled;
        self.active.retain(|j| j.id != job.id);
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
        if let Some(job) = self.active.iter_mut().find(|j| j.id == job_id) {
            job.transferred_bytes = transferred;
            if size.is_some() {
                job.size_bytes = size;
            }
        }
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
}
