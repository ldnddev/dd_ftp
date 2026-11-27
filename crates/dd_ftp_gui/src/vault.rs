//! Password vault for bookmarks. The live vault is [`dd_ftp_storage::SecretStore`].
//! Tests use an in-memory map under the same key the keyring would use.

use std::collections::HashMap;

use anyhow::Result;
use dd_ftp_storage::SecretStore;

#[derive(Clone, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum Vault {
    Live,
    Memory(HashMap<String, String>),
}

impl Vault {
    pub fn live() -> Self {
        Self::Live
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn memory() -> Self {
        Self::Memory(HashMap::new())
    }

    pub fn save(
        &mut self,
        name: &str,
        username: &str,
        host: &str,
        port: u16,
        password: &str,
    ) -> Result<()> {
        match self {
            Self::Live => SecretStore::save_password(name, username, host, port, password),
            Self::Memory(map) => {
                map.insert(
                    SecretStore::primary_key_for(name, username, host, port),
                    password.to_string(),
                );
                Ok(())
            }
        }
    }

    pub fn load(
        &self,
        name: &str,
        username: &str,
        host: &str,
        port: u16,
    ) -> Result<Option<String>> {
        match self {
            Self::Live => SecretStore::load_password(name, username, host, port),
            Self::Memory(map) => Ok(map
                .get(&SecretStore::primary_key_for(name, username, host, port))
                .cloned()),
        }
    }

    pub fn delete(&mut self, name: &str, username: &str, host: &str, port: u16) -> Result<()> {
        match self {
            Self::Live => SecretStore::delete_password(name, username, host, port),
            Self::Memory(map) => {
                map.remove(&SecretStore::primary_key_for(name, username, host, port));
                Ok(())
            }
        }
    }

    pub fn health(&self) -> Result<()> {
        match self {
            Self::Live => SecretStore::check_backend_available(),
            Self::Memory(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_and_underscore_do_not_share_a_secret() {
        let mut vault = Vault::memory();
        vault
            .save("site", "alice@ex.com", "files.example", 21, "one")
            .unwrap();
        vault
            .save("site", "alice_ex.com", "files.example", 21, "two")
            .unwrap();
        assert_eq!(
            vault
                .load("site", "alice@ex.com", "files.example", 21)
                .unwrap()
                .as_deref(),
            Some("one")
        );
        assert_eq!(
            vault
                .load("site", "alice_ex.com", "files.example", 21)
                .unwrap()
                .as_deref(),
            Some("two")
        );
        assert_ne!(
            SecretStore::primary_key_for("site", "alice@ex.com", "files.example", 21),
            SecretStore::primary_key_for("site", "alice_ex.com", "files.example", 21)
        );
    }
}
