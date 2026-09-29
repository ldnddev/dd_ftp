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

    pub fn warning(message: String) -> Self {
        Self::new(message, ToastLevel::Warning)
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

    #[cfg(test)]
    pub fn expired_for_tests(message: String) -> Self {
        Self {
            message,
            level: ToastLevel::Info,
            created_at: Instant::now()
                .checked_sub(TOAST_TTL + Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;

    #[test]
    fn expire_toast_reports_whether_it_cleared() {
        let mut app = AppState {
            toast: Some(Toast::info("live".into())),
            ..Default::default()
        };
        assert!(!app.expire_toast());
        assert!(app.toast.is_some());

        app.toast = Some(Toast::expired_for_tests("gone".into()));
        assert!(app.expire_toast());
        assert!(app.toast.is_none());
        assert!(!app.expire_toast());
    }
}
