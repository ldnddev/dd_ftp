use anyhow::Result;

pub struct SecretStore;

impl SecretStore {
    fn service_name() -> &'static str {
        "dd_ftp"
    }

    fn password_key(site_name: &str, username: &str, host: &str, port: u16) -> String {
        format!("site:{site_name}|user:{username}|host:{host}|port:{port}|password")
    }

    pub fn save_password(site_name: &str, username: &str, host: &str, port: u16, password: &str) -> Result<()> {
        let entry = keyring::Entry::new(Self::service_name(), &Self::password_key(site_name, username, host, port))?;
        entry.set_password(password)?;
        Ok(())
    }

    pub fn load_password(site_name: &str, username: &str, host: &str, port: u16) -> Result<Option<String>> {
        let entry = keyring::Entry::new(Self::service_name(), &Self::password_key(site_name, username, host, port))?;
        match entry.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete_password(site_name: &str, username: &str, host: &str, port: u16) -> Result<()> {
        let entry = keyring::Entry::new(Self::service_name(), &Self::password_key(site_name, username, host, port))?;
        match entry.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
