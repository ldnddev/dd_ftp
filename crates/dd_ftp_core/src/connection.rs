use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Protocol {
    Sftp,
    Ftp,
    Ftps,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub protocol: Protocol,
    pub username: String,
    /// Never written by serde. Bookmarks keep this in the keyring, not in TOML.
    #[serde(default, skip_serializing)]
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub initial_path: String,
}

impl std::fmt::Debug for ConnectionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionInfo")
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("protocol", &self.protocol)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("private_key", &self.private_key)
            .field("initial_path", &self.initial_path)
            .finish()
    }
}

impl Default for ConnectionInfo {
    fn default() -> Self {
        Self {
            name: "New Site".to_string(),
            host: "127.0.0.1".to_string(),
            port: 22,
            protocol: Protocol::Sftp,
            username: String::new(),
            password: None,
            private_key: None,
            initial_path: "/".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_does_not_include_the_password() {
        let info = ConnectionInfo {
            password: Some("s3cret-value".into()),
            ..ConnectionInfo::default()
        };
        let rendered = format!("{info:?}");
        assert!(!rendered.contains("s3cret-value"));
        assert!(rendered.contains("<redacted>"));
    }
}
