pub use dd_ftp_app::keys::{KeyBinding, KeyGroup, KEYMAP};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keymap_keys_appear_in_tutorial_controls() {
        let html = include_str!("../../../docs/index.html");
        let start = html
            .find("id=\"keys\"")
            .expect("docs/index.html must have a keys section");
        let rest = &html[start..];
        let end = rest.find("id=\"uninstall\"").unwrap_or(rest.len());
        let controls = &rest[..end];
        for kb in KEYMAP {
            assert!(
                controls.contains(kb.keys),
                "KEYMAP keys {:?} not found in docs/index.html keys section",
                kb.keys
            );
        }
    }
}
