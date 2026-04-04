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
