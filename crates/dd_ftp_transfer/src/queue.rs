use dd_ftp_core::TransferJob;

#[derive(Debug, Default)]
pub struct TransferQueue {
    pub pending: Vec<TransferJob>,
    pub active: Vec<TransferJob>,
    pub completed: Vec<TransferJob>,
    pub failed: Vec<TransferJob>,
}

impl TransferQueue {
    pub fn enqueue(&mut self, job: TransferJob) {
        self.pending.push(job);
    }

    pub fn next_pending(&mut self) -> Option<TransferJob> {
        if self.pending.is_empty() {
            None
        } else {
            Some(self.pending.remove(0))
        }
    }

    pub fn mark_completed(&mut self, job: TransferJob) {
        self.completed.push(job);
    }

    pub fn mark_failed(&mut self, job: TransferJob) {
        self.failed.push(job);
    }
}
