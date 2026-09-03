use dd_ftp_app::{reduce, Action, AppState};
use dd_ftp_core::ConnectionInfo;
use dd_ftp_storage::{SecretStore, SiteManager};

pub(crate) fn hydrate_password_from_keyring(
    app: &mut AppState,
    mut info: ConnectionInfo,
    context: &str,
) -> ConnectionInfo {
    if info.password.is_none() {
        match SecretStore::load_password(&info.name, &info.username, &info.host, info.port) {
            Ok(Some(secret)) => {
                info.password = Some(secret);
            }
            Ok(None) => {
                reduce(
                    app,
                    Action::SetStatus(format!(
                        "No keyring password found ({context}) for {}@{}:{}",
                        info.username, info.host, info.port
                    )),
                );
            }
            Err(err) => {
                let msg = format!(
                    "Keyring load failed ({context}) for {}@{}:{}: {err}",
                    info.username, info.host, info.port
                );
                reduce(app, Action::SetStatus(msg.clone()));
                reduce(app, Action::ShowError(msg));
            }
        }
    }
    info
}

pub(crate) fn run_keyring_health_check(app: &mut AppState) {
    match SecretStore::check_backend_available() {
        Ok(_) => {
            reduce(
                app,
                Action::SetStatus(
                    "Keyring backend detected: password persistence enabled".to_string(),
                ),
            );
        }
        Err(err) => {
            let msg = format!(
                "Keyring backend unavailable. Password persistence disabled. Details: {err}"
            );
            reduce(app, Action::SetStatus(msg.clone()));
            reduce(app, Action::ShowError(msg));
        }
    }
}

pub(crate) fn save_quick_connect_bookmark(app: &mut AppState) {
    let mut cfg = SiteManager::load_or_default().unwrap_or_default();
    let info = app.quick_connect.clone();

    if info.name.trim().is_empty()
        || info.host.trim().is_empty()
        || info.username.trim().is_empty()
        || info.port == 0
    {
        reduce(
            app,
            Action::SetStatus("Cannot save bookmark: host/user/port required".to_string()),
        );
        return;
    }

    let secret_status = if let Some(password) = info.password.as_deref() {
        if let Err(err) =
            SecretStore::save_password(&info.name, &info.username, &info.host, info.port, password)
        {
            let msg = format!("Save secret failed: {err}");
            reduce(app, Action::SetStatus(msg.clone()));
            reduce(app, Action::ShowError(msg));
            return;
        }

        let key = SecretStore::primary_key_for(&info.name, &info.username, &info.host, info.port);
        match SecretStore::load_password(&info.name, &info.username, &info.host, info.port) {
            Ok(Some(_)) => format!("Password saved to keyring (verified key: {key})"),
            Ok(None) => {
                let msg = format!(
                    "Password save reported success, but verification lookup returned no entry (key: {key})"
                );
                reduce(app, Action::ShowError(msg.clone()));
                msg
            }
            Err(err) => {
                let msg = format!("Password save verification failed for key {key}: {err}");
                reduce(app, Action::ShowError(msg.clone()));
                msg
            }
        }
    } else {
        "No password provided (bookmark saved without keyring secret)".to_string()
    };

    let existing_idx = cfg
        .sites
        .iter()
        .position(|s| s.host == info.host && s.username == info.username && s.port == info.port);

    if let Some(idx) = existing_idx {
        cfg.sites[idx] = info;
        if cfg.default_site.is_none() {
            cfg.default_site = Some(idx);
        }

        match SiteManager::save_to_default_path(&cfg) {
            Ok(_) => {
                app.selected_bookmark = idx;
                reduce(app, Action::SetBookmarks(cfg.sites));
                reduce(
                    app,
                    Action::SetStatus(format!("Updated bookmark | {}", secret_status)),
                );
            }
            Err(err) => {
                reduce(
                    app,
                    Action::SetStatus(format!("Save bookmark failed: {err}")),
                );
            }
        }
    } else {
        cfg.sites.push(info);
        let idx = cfg.sites.len().saturating_sub(1);
        if cfg.default_site.is_none() {
            cfg.default_site = Some(0);
        }

        match SiteManager::save_to_default_path(&cfg) {
            Ok(_) => {
                app.selected_bookmark = idx;
                reduce(app, Action::SetBookmarks(cfg.sites));
                reduce(
                    app,
                    Action::SetStatus(format!("Saved bookmark | {}", secret_status)),
                );
            }
            Err(err) => {
                reduce(
                    app,
                    Action::SetStatus(format!("Save bookmark failed: {err}")),
                );
            }
        }
    }
}

pub(crate) fn delete_bookmark_named(app: &mut AppState, name: &str) {
    let mut cfg = SiteManager::load_or_default().unwrap_or_default();
    if cfg.sites.is_empty() {
        reduce(app, Action::SetStatus("No bookmarks to delete".to_string()));
        return;
    }

    let Some(idx) = cfg.sites.iter().position(|s| s.name == name) else {
        reduce(
            app,
            Action::SetStatus(format!("Bookmark not found: {name}")),
        );
        return;
    };

    let removed = cfg.sites.remove(idx);
    let _ = SecretStore::delete_password(
        &removed.name,
        &removed.username,
        &removed.host,
        removed.port,
    );

    if let Some(default_idx) = cfg.default_site {
        cfg.default_site = if cfg.sites.is_empty() {
            None
        } else if default_idx == idx {
            Some(0)
        } else if default_idx > idx {
            Some(default_idx - 1)
        } else {
            Some(default_idx)
        };
    }

    match SiteManager::save_to_default_path(&cfg) {
        Ok(_) => {
            reduce(app, Action::SetBookmarks(cfg.sites));
            reduce(
                app,
                Action::SetStatus(format!("Deleted bookmark: {}", removed.name)),
            );
        }
        Err(err) => {
            reduce(
                app,
                Action::SetStatus(format!("Delete bookmark failed: {err}")),
            );
        }
    }
}

pub(crate) fn set_default_bookmark(app: &mut AppState) {
    let mut cfg = SiteManager::load_or_default().unwrap_or_default();
    if cfg.sites.is_empty() {
        reduce(
            app,
            Action::SetStatus("No bookmarks to set as default".to_string()),
        );
        return;
    }

    if app.selected_bookmark >= cfg.sites.len() {
        reduce(
            app,
            Action::SetStatus("Invalid bookmark selection".to_string()),
        );
        return;
    }

    let selected = app.selected_bookmark;
    if selected != 0 {
        cfg.sites.swap(0, selected);
    }
    cfg.default_site = Some(0);

    match SiteManager::save_to_default_path(&cfg) {
        Ok(_) => {
            reduce(app, Action::SetBookmarks(cfg.sites));
            reduce(
                app,
                Action::SetStatus("Default bookmark updated".to_string()),
            );
        }
        Err(err) => {
            reduce(app, Action::SetStatus(format!("Set default failed: {err}")));
        }
    }
}
