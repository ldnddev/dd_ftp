use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Protocol {
    Sftp,
    Ftp,
    Ftps,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub protocol: Protocol,
    pub username: String,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub initial_path: String,
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
