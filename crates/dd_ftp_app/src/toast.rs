use std::time::{Duration, Instant};

pub const TOAST_TTL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug)]
pub struct Toast {
    pub message: String,
    pub level: ToastLevel,
    created_at: Instant,
}

impl Toast {
    pub fn info(message: String) -> Self {
        Self::new(message, ToastLevel::Info)
    }

    pub fn error(message: String) -> Self {
        Self::new(message, ToastLevel::Error)
    }

    fn new(message: String, level: ToastLevel) -> Self {
        Self {
            message,
            level,
            created_at: Instant::now(),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= TOAST_TTL
    }
}
