use std::collections::HashSet;

use anyhow::Result;

pub struct SecretStore;

impl SecretStore {
    fn probe_user() -> &'static str {
        "ddftp_probe_user"
    }

    fn service_name() -> &'static str {
        "dd_ftp"
    }

    fn normalize_host(host: &str) -> String {
        host.trim().to_lowercase()
    }

    fn normalize_user(username: &str) -> String {
        username.trim().to_string()
    }

    fn sanitize_component(input: &str) -> String {
        input
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    // v3 (stable): independent from editable bookmark label/name and avoids special delimiters.
    fn password_key(username: &str, host: &str, port: u16) -> String {
        let u = Self::sanitize_component(username);
        let h = Self::sanitize_component(host);
        format!("ddftp-{u}-{h}-{port}")
    }

    // v1 (legacy): included site_name; retained for fallback migration.
    fn legacy_password_key(site_name: &str, username: &str, host: &str, port: u16) -> String {
        format!("site:{site_name}|user:{username}|host:{host}|port:{port}|password")
    }

    fn candidate_keys(site_name: &str, username: &str, host: &str, port: u16) -> Vec<String> {
        let user = Self::normalize_user(username);
        let host_raw = host.trim().to_string();
        let host_norm = Self::normalize_host(host);

        let mut keys = vec![
            // v3 key format
            Self::password_key(&user, &host_raw, port),
            Self::password_key(&user, &host_norm, port),
            // v2 key format (kept for migration)
            format!("user:{user}|host:{host_raw}|port:{port}|password"),
            format!("user:{user}|host:{host_norm}|port:{port}|password"),
            // v1 key format (kept for migration)
            Self::legacy_password_key(site_name, &user, &host_raw, port),
            Self::legacy_password_key(site_name, &user, &host_norm, port),
        ];

        // Deduplicate if raw host == normalized host.
        let mut seen = HashSet::new();
        keys.retain(|k| seen.insert(k.clone()));
        keys
    }

    pub fn save_password(
        site_name: &str,
        username: &str,
        host: &str,
        port: u16,
        password: &str,
    ) -> Result<()> {
        let keys = Self::candidate_keys(site_name, username, host, port);
        let primary_key = keys
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no key candidates generated"))?;

        // Write only the primary key to avoid backend-specific collisions on alias keys.
        let primary = keyring::Entry::new(Self::service_name(), &primary_key)?;
        primary.set_password(password)?;

        // Immediate direct verification on the same key.
        let verify = primary.get_password()?;
        if verify != password {
            anyhow::bail!("keyring verification mismatch for key {primary_key}");
        }

        Ok(())
    }

    pub fn load_password(site_name: &str, username: &str, host: &str, port: u16) -> Result<Option<String>> {
        let keys = Self::candidate_keys(site_name, username, host, port);
        let primary_key = keys
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no key candidates generated"))?;

        let primary = keyring::Entry::new(Self::service_name(), &primary_key)?;

        for key in keys {
            let entry = keyring::Entry::new(Self::service_name(), &key)?;
            match entry.get_password() {
                Ok(v) => {
                    // Migrate aliases to primary key.
                    if key != primary_key {
                        let _ = primary.set_password(&v);
                    }
                    return Ok(Some(v));
                }
                Err(keyring::Error::NoEntry) => continue,
                Err(e) => return Err(e.into()),
            }
        }

        Ok(None)
    }

    pub fn delete_password(site_name: &str, username: &str, host: &str, port: u16) -> Result<()> {
        for key in Self::candidate_keys(site_name, username, host, port) {
            let entry = keyring::Entry::new(Self::service_name(), &key)?;
            match entry.delete_credential() {
                Ok(_) | Err(keyring::Error::NoEntry) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    pub fn primary_key_for(site_name: &str, username: &str, host: &str, port: u16) -> String {
        Self::candidate_keys(site_name, username, host, port)
            .into_iter()
            .next()
            .unwrap_or_else(|| "ddftp-invalid-key".to_string())
    }

    pub fn check_backend_available() -> Result<()> {
        let probe = keyring::Entry::new(Self::service_name(), Self::probe_user())?;
        let token = "ok";

        probe.set_password(token)?;
        let loaded = probe.get_password()?;
        if loaded != token {
            anyhow::bail!("keyring probe mismatch");
        }

        let _ = probe.delete_credential();
        Ok(())
    }
}
