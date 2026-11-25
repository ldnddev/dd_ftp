//! Window tests. Each case mounts the shell in GPUI's `TestAppContext`.

use std::path::Path;

use dd_ftp_core::{ConnectionInfo, Protocol};
use gpui::{AppContext, TestAppContext};
use ldnddev_theme::OmarchyPaths;

use crate::shell::{Shell, ShellOptions};

fn opts(dir: &Path) -> ShellOptions {
    ShellOptions {
        local_cwd: dir.to_path_buf(),
        gui_path: dir.join("gui.json"),
        app_config_path: dir.join("dd_ftp.toml"),
        load_app_config: false,
        sites: Vec::new(),
        load_user_sites: false,
        project_root: dir.to_path_buf(),
        home: Some(dir.join("home")),
        config_home: Some(dir.join("config")),
        omarchy: OmarchyPaths::from_root(&dir.join("omarchy")),
        tick: false,
        quit_runtime: false,
    }
}

fn mount(cx: &mut TestAppContext, options: ShellOptions) -> gpui::Entity<Shell> {
    cx.update(gpui_component::init);
    cx.new(|cx| Shell::open(options, cx))
}

fn flow_theme(root: &Path) {
    let dir = root.join("stock").join("flow");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("colors.toml"),
        "\
color0 = \"#101010\"
color1 = \"#ff0000\"
color2 = \"#00ff00\"
color3 = \"#ffff00\"
color4 = \"#0000ff\"
color5 = \"#ff00ff\"
color6 = \"#00ffff\"
color7 = \"#eeeeee\"
",
    )
    .unwrap();
}

#[gpui::test]
fn settings_sheet_pages_and_second_f1_restores_the_palette(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let shell = mount(cx, opts(dir.path()));
    shell.update(cx, |shell, cx| {
        let before = shell.palette_digest();
        shell.handle_key("f1", cx);
        let text = shell.visible_text();
        assert!(text.contains("settings-panel:right:70"), "{text}");
        assert!(
            text.contains("settings-nav:Themes,Window,Session,Help"),
            "{text}"
        );
        assert!(text.contains("settings-page:Themes"), "{text}");
        assert!(
            text.contains("theme-list:flow") || text.contains("Theme color editor"),
            "{text}"
        );
        assert!(text.contains("Theme color editor"), "{text}");
        assert!(text.contains("footer:F1: Settings  Esc: Close  Ctrl+Q: Quit"), "{text}");
        shell.handle_key("right", cx);
        assert_ne!(shell.palette_digest(), before, "a nudge paints a preview");
        shell.handle_key("f1", cx);
        let text = shell.visible_text();
        assert!(!text.contains("settings-panel:right:70"), "{text}");
        assert_eq!(shell.palette_digest(), before);
        assert!(text.contains("footer:F1: Settings  Ctrl+Q: Quit"), "{text}");

        shell.handle_key("f2", cx);
        assert!(shell.visible_text().contains("settings-page:Themes"));
        shell.handle_key("ctrl-/", cx);
        assert!(!shell.visible_text().contains("settings-panel:right:70"));
        shell.handle_key("ctrl-,", cx);
        assert!(shell.visible_text().contains("settings-panel:right:70"));
        shell.handle_key("escape", cx);
        assert!(!shell.visible_text().contains("settings-panel:right:70"));
        assert_eq!(shell.palette_digest(), before);

        shell.handle_key("f3", cx);
        let text = shell.visible_text();
        assert!(text.contains("settings-panel:right:70"), "{text}");
        assert!(text.contains("settings-page:Session"), "{text}");
        assert!(text.contains("Workers"), "{text}");
        assert!(text.contains("Hide dotfiles"), "{text}");
        assert!(text.contains("Editor"), "{text}");
        assert!(text.contains("Overwrite"), "{text}");
        assert!(text.contains("Auto-connect"), "{text}");
        assert!(text.contains("Last local path"), "{text}");

        shell.handle_key("tab", cx);
        shell.handle_key("right", cx);
        assert!(shell.visible_text().contains("Workers 3"), "{}", shell.visible_text());
        shell.handle_key("tab", cx);
        shell.handle_key("space", cx);
        assert!(shell.visible_text().contains("Hide dotfiles on"));

        shell.handle_key("]", cx);
        let text = shell.visible_text();
        assert!(text.contains("settings-page:Help"), "{text}");
        assert!(text.contains("Settings · F1 · F1"), "{text}");
        assert!(text.contains("Session · F3 · F3"), "{text}");

        shell.handle_key("f1", cx);
        assert!(!shell.visible_text().contains("settings-panel:right:70"));
        shell.handle_key("f1", cx);
        shell.handle_key("tab", cx);
        let text = shell.visible_text();
        assert!(text.contains("settings-page:Window"), "{text}");
        assert!(text.contains("Density"), "{text}");
        assert!(text.contains("Vim"), "{text}");
        assert!(text.contains("Density Comfortable"), "{text}");
        assert!(text.contains("Vim keys on"), "{text}");
    });
}

#[gpui::test]
fn theme_list_steps_back_to_the_list_before_close(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let omarchy = dir.path().join("omarchy");
    flow_theme(&omarchy);
    let mut options = opts(dir.path());
    options.omarchy = OmarchyPaths::from_root(&omarchy);
    let shell = mount(cx, options);
    shell.update(cx, |shell, cx| {
        let before = shell.palette_digest();
        shell.handle_key("f1", cx);
        let text = shell.visible_text();
        assert!(text.contains("theme-list:flow"), "{text}");
        assert!(!text.contains("Theme color editor"), "{text}");
        shell.handle_key("down", cx);
        shell.handle_key("e", cx);
        let text = shell.visible_text();
        assert!(text.contains("Theme color editor"), "{text}");
        assert!(text.contains("settings-panel:right:70"), "{text}");
        shell.handle_key("escape", cx);
        let text = shell.visible_text();
        assert!(text.contains("theme-list:flow"), "{text}");
        assert!(!text.contains("Theme color editor"), "{text}");
        assert!(text.contains("settings-panel:right:70"), "{text}");
        shell.handle_key("escape", cx);
        let text = shell.visible_text();
        assert!(!text.contains("settings-panel:right:70"), "{text}");
        assert_eq!(shell.palette_digest(), before);
    });
}

#[gpui::test]
fn body_lists_a_directory_and_connect_toasts_the_host(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let files = dir.path().join("files");
    std::fs::create_dir_all(files.join("nested")).unwrap();
    std::fs::write(files.join("alpha.txt"), b"a").unwrap();
    std::fs::write(files.join("beta.txt"), b"bb").unwrap();
    let mut options = opts(dir.path());
    options.local_cwd = files;
    options.sites = vec![ConnectionInfo {
        name: "Desk".into(),
        host: "files.example".into(),
        port: 22,
        protocol: Protocol::Sftp,
        username: "ada".into(),
        password: Some("s3cret-value".into()),
        ..ConnectionInfo::default()
    }];
    let shell = mount(cx, options);
    shell.update(cx, |shell, cx| {
        let text = shell.visible_text();
        assert!(text.contains("alpha.txt"), "{text}");
        assert!(text.contains("beta.txt"), "{text}");
        assert!(text.contains("nested"), "{text}");
        assert!(text.contains("dir"), "{text}");
        assert!(text.contains("files.example"), "{text}");
        assert!(!text.contains("s3cret-value"), "{text}");
        assert!(!text.contains("password"), "{text}");

        shell.handle_key("down", cx);
        shell.handle_key("enter", cx);
        assert!(
            shell.visible_text().contains("alpha.txt") || shell.visible_text().contains(".."),
            "{}",
            shell.visible_text()
        );

        shell.handle_key("tab", cx);
        shell.handle_key("tab", cx);
        for ch in ["e", "x", "a", "m", "p", "l", "e", ".", "t", "e", "s", "t"] {
            shell.handle_key(ch, cx);
        }
        shell.handle_key("enter", cx);
        for _ in 0..50 {
            shell.poll_jobs();
            if shell.visible_text().contains("toast:Connect example.test:22 (sftp)") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let text = shell.visible_text();
        assert!(
            text.contains("toast:Connect example.test:22 (sftp)"),
            "{text}"
        );
        assert!(!text.contains("s3cret-value"), "{text}");
    });
}

#[gpui::test]
fn gui_json_round_trip_does_not_rewrite_dd_ftp_toml(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("dd_ftp.toml");
    std::fs::write(&toml_path, "editor = \"hx\"\n").unwrap();
    let before = std::fs::read(&toml_path).unwrap();
    let shell = mount(cx, opts(dir.path()));
    shell.update(cx, |shell, cx| {
        shell.handle_key("f1", cx);
        shell.handle_key("tab", cx);
        assert!(shell.visible_text().contains("settings-page:Window"));
        shell.handle_key("space", cx);
        let gui = std::fs::read_to_string(dir.path().join("gui.json")).unwrap();
        assert!(gui.contains("compact"), "{gui}");
        assert!(gui.contains("vim_keys"), "{gui}");
        assert_eq!(std::fs::read(&toml_path).unwrap(), before);
        assert!(!dir.path().join("dd_ftp_theme.yml").exists());
        assert!(!dir.path().join("config").join("dd_ftp_theme.yml").exists());

        shell.handle_key("f3", cx);
        shell.handle_key("enter", cx);
        let saved = std::fs::read_to_string(&toml_path).unwrap();
        assert!(saved.contains("worker_max_concurrency"), "{saved}");
        assert_ne!(std::fs::read(&toml_path).unwrap(), before);
        let gui_after = std::fs::read_to_string(dir.path().join("gui.json")).unwrap();
        assert_eq!(gui_after, gui);
    });
}

#[gpui::test]
fn ctrl_q_confirms_during_a_transfer_and_bare_q_does_not_quit(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let shell = mount(cx, opts(dir.path()));
    shell.update(cx, |shell, cx| {
        shell.handle_key("q", cx);
        assert!(!shell.quit);
        assert!(!shell.visible_text().contains("Quit while a transfer"));
        shell.set_transfer_active(true);
        assert!(shell.visible_text().contains('▃') || shell.visible_text().contains('▅') || shell.visible_text().contains('█'));
        shell.handle_key("q", cx);
        assert!(!shell.quit);
        shell.handle_key("ctrl-q", cx);
        let text = shell.visible_text();
        assert!(text.contains("Quit while a transfer is active?"), "{text}");
        assert!(text.contains("Esc: Close"), "{text}");
        shell.handle_key("escape", cx);
        assert!(!shell.quit);
        shell.handle_key("ctrl-q", cx);
        shell.handle_key("enter", cx);
        assert!(shell.quit);
    });
}

#[test]
fn help_flag_text_names_the_window() {
    let help = include_str!("main.rs");
    assert!(help.contains("dd_ftp-gui [--help]"));
    assert!(help.contains("F3"));
}
