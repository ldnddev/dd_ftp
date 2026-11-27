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
        apply_env: false,
        backend: crate::session::Backend::fake(crate::session::FakeTree::empty()),
        sites_path: dir.join("sites.toml"),
        default_site: None,
        vault: crate::vault::Vault::memory(),
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
        assert!(
            text.contains("footer:F1: Settings  Esc: Close  Ctrl+Q: Quit"),
            "{text}"
        );
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
        assert!(
            shell.visible_text().contains("Workers 3"),
            "{}",
            shell.visible_text()
        );
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
        assert!(text.contains("remote:disconnected"), "{text}");
        assert!(text.contains("pass:"), "{text}");
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
        shell.handle_key("enter", cx);
        let text = shell.visible_text();
        assert!(text.contains("host:files.example"), "{text}");
        assert!(text.contains("pass:************"), "{text}");
        assert!(text.contains("toast:Bookmark files.example"), "{text}");
        assert!(!text.contains("s3cret-value"), "{text}");
        assert!(!text.contains("password"), "{text}");
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
        assert!(
            shell.visible_text().contains('▃')
                || shell.visible_text().contains('▅')
                || shell.visible_text().contains('█')
        );
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

fn selected_name(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let rest = line.strip_prefix('>')?;
        let rest = rest.strip_prefix('*').or_else(|| rest.strip_prefix(' '))?;
        Some(rest.trim_start().split("  ").next()?.trim().to_string())
    })
}

fn row_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix('>').or_else(|| line.strip_prefix(' '))?;
            let rest = rest.strip_prefix('*').or_else(|| rest.strip_prefix(' '))?;
            let name = rest.trim_start().split("  ").next()?.trim();
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect()
}

#[gpui::test]
fn local_browser_filters_sorts_marks_and_walks_the_path(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let nested = root.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(root.join("alpha.txt"), b"a").unwrap();
    std::fs::write(root.join("beta.txt"), b"bbb").unwrap();
    std::fs::write(root.join(".secret"), b"s").unwrap();
    let many = root.join("many");
    std::fs::create_dir(&many).unwrap();
    for index in 0..15 {
        std::fs::write(many.join(format!("f{index:02}")), b"x").unwrap();
    }
    let mut options = opts(dir.path());
    options.local_cwd = root.clone();
    let shell = mount(cx, options);
    shell.update(cx, |shell, cx| {
        let text = shell.visible_text();
        assert!(text.contains("sort:name asc"), "{text}");
        assert!(
            text.contains("\nfilter:\n") || text.ends_with("filter:"),
            "{text}"
        );
        assert!(text.contains("path-bar:"), "{text}");
        assert!(text.contains("root=>"), "{text}");
        assert_eq!(
            row_names(&text),
            vec![
                ".",
                "..",
                "many",
                "nested",
                ".secret",
                "alpha.txt",
                "beta.txt"
            ]
        );

        shell.handle_key(".", cx);
        let hidden = shell.visible_text();
        assert!(!hidden.contains(".secret"), "{hidden}");
        assert_eq!(
            row_names(&hidden),
            vec![".", "..", "many", "nested", "alpha.txt", "beta.txt"]
        );

        shell.handle_key("/", cx);
        shell.handle_key("a", cx);
        shell.handle_key("l", cx);
        shell.handle_key("p", cx);
        let filtering = shell.visible_text();
        assert!(filtering.contains("filter-open:alp"), "{filtering}");
        assert_eq!(row_names(&filtering), vec!["alpha.txt"]);
        shell.handle_key("enter", cx);
        let kept = shell.visible_text();
        assert!(
            kept.contains("\nfilter:alp\n") || kept.ends_with("filter:alp"),
            "{kept}"
        );
        assert!(!kept.contains("filter-open"), "{kept}");
        shell.handle_key("/", cx);
        shell.handle_key("escape", cx);
        let cleared = shell.visible_text();
        assert!(
            cleared.contains("alpha.txt") && cleared.contains("beta.txt"),
            "{cleared}"
        );
        assert!(!cleared.contains(".secret"), "{cleared}");
        assert!(!cleared.contains("filter-open"), "{cleared}");

        shell.handle_key("s", cx);
        let by_size = shell.visible_text();
        assert!(by_size.contains("sort:size asc"), "{by_size}");
        let sized = row_names(&by_size);
        let alpha = sized.iter().position(|name| name == "alpha.txt").unwrap();
        let beta = sized.iter().position(|name| name == "beta.txt").unwrap();
        assert!(alpha < beta, "{sized:?}");
        shell.handle_key("shift-s", cx);
        let flipped = row_names(&shell.visible_text());
        let alpha = flipped.iter().position(|name| name == "alpha.txt").unwrap();
        let beta = flipped.iter().position(|name| name == "beta.txt").unwrap();
        assert!(beta < alpha, "{flipped:?}");
        assert!(shell.visible_text().contains("sort:size desc"));
        shell.handle_key("shift-s", cx);
        shell.handle_key("s", cx);
        shell.handle_key("s", cx);
        assert!(
            shell.visible_text().contains("sort:name asc"),
            "{}",
            shell.visible_text()
        );

        shell.handle_key("home", cx);
        shell.handle_key("space", cx);
        let dot = shell.visible_text();
        assert!(dot.lines().any(|line| line.starts_with(">  .  ")), "{dot}");
        shell.handle_key("end", cx);
        shell.handle_key("space", cx);
        assert!(shell.visible_text().contains("* beta.txt"),);
        shell.handle_key("shift-up", cx);
        assert!(
            shell.visible_text().contains("* alpha.txt"),
            "{text}",
            text = shell.visible_text()
        );
        assert!(shell.visible_text().contains("* beta.txt"));
        shell.handle_key("ctrl-a", cx);
        let marked = shell.visible_text();
        assert!(marked.contains("* many"), "{marked}");
        assert!(marked.contains("* nested"), "{marked}");
        assert!(marked.contains("* alpha.txt"), "{marked}");
        assert!(marked.contains("* beta.txt"), "{marked}");
        assert!(
            marked.lines().any(|line| line.contains("  .  0  dir")),
            "{marked}"
        );
        assert!(
            !marked
                .lines()
                .any(|line| line.contains("* .  ") || line.contains("* ..")),
            "{marked}"
        );
        shell.handle_key("escape", cx);
        let unmarked = shell.visible_text();
        assert!(!unmarked.contains('*'), "{unmarked}");

        shell.handle_key("home", cx);
        shell.handle_key("pagedown", cx);
        assert_ne!(selected_name(&shell.visible_text()).as_deref(), Some("."));
        shell.handle_key("home", cx);
        assert_eq!(selected_name(&shell.visible_text()).as_deref(), Some("."));
        shell.handle_key("end", cx);
        assert_eq!(
            selected_name(&shell.visible_text()).as_deref(),
            Some("beta.txt")
        );

        let before = shell.visible_text();
        let bar = before
            .lines()
            .find(|line| line.starts_with("path-bar:"))
            .unwrap();
        let segments = bar.trim_start_matches("path-bar:").split('|').count();
        shell.activate_path_segment(segments - 1, true);
        let parent = shell.visible_text();
        assert!(
            parent.contains("local:") && parent.contains("root"),
            "{parent}"
        );
        assert!(
            parent
                .lines()
                .any(|line| line.starts_with("local:") && !line.ends_with("/root")),
            "{parent}"
        );
        std::fs::write(dir.path().join("fresh.txt"), b"f").unwrap();
        shell.handle_key("r", cx);
        assert!(
            shell.visible_text().contains("fresh.txt"),
            "{}",
            shell.visible_text()
        );

        shell.handle_key("h", cx);
        let above = shell.visible_text();
        assert!(above.contains("root") || above.contains("."), "{above}");
    });

    let shell = mount(
        cx,
        ShellOptions {
            local_cwd: many,
            ..opts(dir.path())
        },
    );
    shell.update(cx, |shell, cx| {
        shell.handle_key("end", cx);
        assert_eq!(selected_name(&shell.visible_text()).as_deref(), Some("f14"));
        shell.handle_key("pageup", cx);
        assert_eq!(selected_name(&shell.visible_text()).as_deref(), Some("f04"));
        shell.handle_key("home", cx);
        assert_eq!(selected_name(&shell.visible_text()).as_deref(), Some("."));
        shell.handle_key("pagedown", cx);
        assert_eq!(selected_name(&shell.visible_text()).as_deref(), Some("f08"));
        let _ = cx;
    });
}

fn until(shell: &mut Shell, needle: &str) -> String {
    let mut last = String::new();
    for _ in 0..50 {
        shell.poll_jobs();
        last = shell.visible_text();
        if last.contains(needle) {
            return last;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    last
}

fn secret_form(protocol: Protocol) -> ConnectionInfo {
    ConnectionInfo {
        name: "Desk".into(),
        host: "files.example".into(),
        port: 22,
        protocol,
        username: "ada".into(),
        password: Some("s3cret-value".into()),
        private_key: None,
        initial_path: "/".into(),
    }
}

fn sample_tree(host_key: crate::session::FakeHostKey) -> crate::session::FakeTree {
    let mut dirs = std::collections::HashMap::new();
    dirs.insert(
        "/".into(),
        vec![
            crate::session::entry("docs", true),
            crate::session::entry("readme.txt", false),
        ],
    );
    dirs.insert(
        "/docs".into(),
        vec![crate::session::entry("notes.txt", false)],
    );
    let mut tree = crate::session::FakeTree::empty();
    tree.dirs = dirs;
    tree.host_key = host_key;
    tree.extra_on_refresh = Some(crate::session::entry("fresh.txt", false));
    tree
}

#[gpui::test]
fn session_connects_lists_changes_directory_and_disconnects(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut options = opts(dir.path());
    options.backend =
        crate::session::Backend::fake(sample_tree(crate::session::FakeHostKey::Ask {
            fingerprint: "abc123".into(),
            changed: false,
        }));
    let shell = mount(cx, options);
    shell.update(cx, |shell, cx| {
        shell.fill_quick_connect(&secret_form(Protocol::Sftp));
        shell.handle_key("c", cx);
        let busy = shell.visible_text();
        assert!(busy.contains("busy: connecting"), "{busy}");
        assert!(busy.contains("pass:************"), "{busy}");
        assert!(!busy.contains("s3cret-value"), "{busy}");
        assert!(!busy.contains("password"), "{busy}");

        let prompted = until(shell, "host-key:abc123");
        assert!(prompted.contains("changed:false"), "{prompted}");
        assert!(
            prompted.contains("busy: waiting for host key"),
            "{prompted}"
        );
        shell.handle_key("y", cx);
        let connected = until(shell, "Connected via SFTP to files.example as ada");
        assert!(connected.contains("remote:/"), "{connected}");
        assert!(connected.contains("readme.txt"), "{connected}");
        assert!(connected.contains("docs"), "{connected}");
        assert!(!connected.contains("s3cret-value"), "{connected}");

        shell.handle_key("2", cx);
        shell.handle_key("r", cx);
        let refreshed = until(shell, "fresh.txt");
        assert!(
            refreshed.lines().any(|line| line.contains("fresh.txt")),
            "{refreshed}"
        );

        shell.handle_key("down", cx);
        shell.handle_key("down", cx);
        shell.handle_key("enter", cx);
        let nested = until(shell, "notes.txt");
        assert!(nested.contains("remote:/docs"), "{nested}");
        assert!(!nested.contains("readme.txt"), "{nested}");

        shell.handle_key("backspace", cx);
        let parent = until(shell, "readme.txt");
        assert!(parent.lines().any(|line| line == "remote:/"), "{parent}");
        assert!(!parent.contains("notes.txt"), "{parent}");

        shell.push_stale_listing("evil-stale");
        let stale = shell.visible_text();
        assert!(!stale.contains("evil-stale"), "{stale}");

        shell.handle_key("c", cx);
        let gone = shell.visible_text();
        assert!(gone.contains("remote:disconnected"), "{gone}");
        assert!(gone.contains("status:Disconnected"), "{gone}");
        assert!(!gone.contains("s3cret-value"), "{gone}");
        assert!(!gone.contains("password"), "{gone}");
    });
}

#[gpui::test]
fn refused_host_key_leaves_the_pane_disconnected(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut options = opts(dir.path());
    options.backend =
        crate::session::Backend::fake(sample_tree(crate::session::FakeHostKey::Ask {
            fingerprint: "abc123".into(),
            changed: true,
        }));
    let shell = mount(cx, options);
    shell.update(cx, |shell, cx| {
        shell.fill_quick_connect(&secret_form(Protocol::Sftp));
        shell.handle_key("c", cx);
        let prompted = until(shell, "host-key:abc123");
        assert!(prompted.contains("changed:true"), "{prompted}");
        shell.handle_key("n", cx);
        let rejected = until(shell, "host key rejected");
        assert!(rejected.contains("remote:disconnected"), "{rejected}");
        assert!(!rejected.contains("Connected via"), "{rejected}");
        assert!(!rejected.contains("readme.txt"), "{rejected}");
        assert!(!rejected.contains("s3cret-value"), "{rejected}");
        assert!(!rejected.contains("password"), "{rejected}");
    });
}

#[gpui::test]
fn plain_ftp_status_says_unencrypted_and_ftps_does_not(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let shell = mount(cx, opts(dir.path()));
    shell.update(cx, |shell, cx| {
        shell.fill_quick_connect(&secret_form(Protocol::Ftp));
        shell.handle_key("c", cx);
        let ftp = until(shell, "FTP (unencrypted)");
        assert!(ftp.contains("remote:/"), "{ftp}");
        assert!(!ftp.contains("s3cret-value"), "{ftp}");
        shell.handle_key("c", cx);
        assert!(shell.visible_text().contains("remote:disconnected"));

        shell.fill_quick_connect(&secret_form(Protocol::Ftps));
        shell.handle_key("c", cx);
        let ftps = until(shell, "Connected via FTPS");
        assert!(!ftps.contains("unencrypted"), "{ftps}");
        assert!(ftps.contains("remote:/"), "{ftps}");
        assert!(!ftps.contains("s3cret-value"), "{ftps}");
    });
}

#[gpui::test]
fn launch_env_fills_the_form_and_drops_the_password(cx: &mut TestAppContext) {
    let _guard = crate::session::ENV_LOCK.lock().unwrap();
    let keys = [
        "DD_FTP_HOST",
        "DD_FTP_NAME",
        "DD_FTP_PORT",
        "DD_FTP_USER",
        "DD_FTP_PASS",
        "DD_FTP_KEY",
        "DD_FTP_PATH",
    ];
    let saved: Vec<(String, Option<String>)> = keys
        .iter()
        .map(|key| ((*key).to_string(), std::env::var(key).ok()))
        .collect();
    crate::session::set_test_var("DD_FTP_HOST", "env.example");
    crate::session::set_test_var("DD_FTP_NAME", "Env");
    crate::session::set_test_var("DD_FTP_PORT", "2200");
    crate::session::set_test_var("DD_FTP_USER", "ada");
    crate::session::set_test_var("DD_FTP_PASS", "s3cret-value");
    crate::session::set_test_var("DD_FTP_KEY", "/keys/id");
    crate::session::set_test_var("DD_FTP_PATH", "/inbox");
    let dir = tempfile::tempdir().unwrap();
    let mut options = opts(dir.path());
    options.apply_env = true;
    let shell = mount(cx, options);
    shell.update(cx, |shell, _| {
        let text = shell.visible_text();
        assert!(text.contains("name:Env"), "{text}");
        assert!(text.contains("host:env.example"), "{text}");
        assert!(text.contains("port:2200"), "{text}");
        assert!(text.contains("user:ada"), "{text}");
        assert!(text.contains("pass:************"), "{text}");
        assert!(text.contains("key:/keys/id"), "{text}");
        assert!(text.contains("path:/inbox"), "{text}");
        assert!(text.contains("protocol:sftp"), "{text}");
        assert!(!text.contains("s3cret-value"), "{text}");
        assert!(!text.contains("password"), "{text}");
    });
    assert!(std::env::var("DD_FTP_PASS").is_err());
    for (key, value) in saved {
        match value {
            Some(value) => crate::session::set_test_var(&key, &value),
            None => crate::session::remove_test_var(&key),
        }
    }
}

#[gpui::test]
fn bookmark_round_trip_keeps_the_password_out_of_sites_toml(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let shell = mount(cx, opts(dir.path()));
    shell.update(cx, |shell, cx| {
        shell.fill_quick_connect(&secret_form(Protocol::Sftp));
        shell.handle_key("shift-b", cx);
        let text = shell.visible_text();
        assert!(text.contains("Saved bookmark"), "{text}");
        assert!(text.contains("pass:************"), "{text}");
        assert!(!text.contains("s3cret-value"), "{text}");
        assert_eq!(shell.stored_secret().as_deref(), Some("s3cret-value"));
        let _ = cx;
    });
    let file = std::fs::read_to_string(dir.path().join("sites.toml")).unwrap();
    assert!(file.contains("files.example"), "{file}");
    assert!(!file.contains("s3cret-value"), "{file}");
    assert!(!file.contains("password"), "{file}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("sites.toml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    let mut vault = crate::vault::Vault::memory();
    vault
        .save("Desk", "ada", "files.example", 22, "s3cret-value")
        .unwrap();
    let shell = mount(
        cx,
        ShellOptions {
            load_user_sites: true,
            vault,
            ..opts(dir.path())
        },
    );
    shell.update(cx, |shell, cx| {
        let text = shell.visible_text();
        assert!(text.contains("host:files.example"), "{text}");
        assert!(text.contains("pass:************"), "{text}");
        assert!(text.contains("bookmark-row:"), "{text}");
        assert!(!text.contains("s3cret-value"), "{text}");
        assert!(!text.contains("password"), "{text}");
        shell.handle_key("tab", cx);
        shell.handle_key("tab", cx);
        shell.handle_key("d", cx);
        assert!(shell.visible_text().contains("confirm:delete Desk"));
        shell.handle_key("n", cx);
        assert!(!shell.visible_text().contains("confirm:delete"));
        shell.handle_key("d", cx);
        shell.handle_key("y", cx);
        let text = shell.visible_text();
        assert!(text.contains("Deleted bookmark: Desk"), "{text}");
        assert!(!text.contains("s3cret-value"), "{text}");
        assert!(shell.stored_secret().is_none());
        let _ = cx;
    });
    let file = std::fs::read_to_string(dir.path().join("sites.toml")).unwrap();
    assert!(!file.contains("files.example"), "{file}");
    assert!(!file.contains("s3cret-value"), "{file}");
}

#[gpui::test]
fn bookmark_sets_the_default_connects_and_edits(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut options = opts(dir.path());
    options.backend =
        crate::session::Backend::fake(sample_tree(crate::session::FakeHostKey::Trusted));
    let shell = mount(cx, options);
    shell.update(cx, |shell, cx| {
        shell.fill_quick_connect(&secret_form(Protocol::Sftp));
        shell.handle_key("shift-b", cx);
        shell.fill_quick_connect(&ConnectionInfo {
            name: "Other".into(),
            host: "other.example".into(),
            username: "ada".into(),
            password: Some("s3cret-value".into()),
            ..secret_form(Protocol::Sftp)
        });
        shell.handle_key("shift-b", cx);
        shell.handle_key("tab", cx);
        shell.handle_key("tab", cx);
        shell.handle_key("shift-d", cx);
        let text = shell.visible_text();
        assert!(text.contains("Default bookmark updated"), "{text}");
        assert!(text.contains("default:0"), "{text}");
        assert!(text.contains("bookmark-row:>* Other"), "{text}");
        assert!(!text.contains("s3cret-value"), "{text}");
        shell.handle_key("c", cx);
        let text = until(shell, "Connected via SFTP");
        assert!(text.contains("readme.txt"), "{text}");
        assert!(!text.contains("s3cret-value"), "{text}");
        shell.handle_key("e", cx);
        let text = shell.visible_text();
        assert!(text.contains("host:other.example"), "{text}");
        assert!(text.contains("pass:************"), "{text}");
        assert!(text.contains("toast:Bookmark other.example"), "{text}");
        let file = std::fs::read_to_string(dir.path().join("sites.toml")).unwrap();
        assert!(!file.contains("s3cret-value"), "{file}");
        assert!(!file.contains("password"), "{file}");
        let _ = cx;
    });
}

#[gpui::test]
fn key_picker_selects_a_private_key_and_ctrl_k_reports_the_vault(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("id_ed25519"), b"priv").unwrap();
    std::fs::write(home.join("id_ed25519.pub"), b"pub").unwrap();
    let shell = mount(cx, opts(dir.path()));
    shell.update(cx, |shell, cx| {
        for _ in 0..8 {
            shell.handle_key("tab", cx);
        }
        shell.handle_key("ctrl-p", cx);
        let text = shell.visible_text();
        assert!(text.contains("key-picker:"), "{text}");
        assert!(text.contains("key-row:"), "{text}");
        assert!(text.contains("id_ed25519"), "{text}");
        shell.handle_key("down", cx);
        shell.handle_key("down", cx);
        shell.handle_key("enter", cx);
        let text = shell.visible_text();
        assert!(text.contains("key:"), "{text}");
        assert!(text.contains("id_ed25519"), "{text}");
        assert!(!text.contains("key-picker:"), "{text}");
        shell.handle_key("ctrl-p", cx);
        shell.handle_key("down", cx);
        shell.handle_key("enter", cx);
        let text = shell.visible_text();
        assert!(text.contains("public key"), "{text}");
        assert!(text.contains(".pub"), "{text}");
        shell.handle_key("ctrl-k", cx);
        let text = shell.visible_text();
        assert!(
            text.contains("Keyring backend detected: password persistence enabled"),
            "{text}"
        );
        let _ = cx;
    });
}

#[gpui::test]
fn auto_connect_uses_the_default_site(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let site = ConnectionInfo {
        password: None,
        ..secret_form(Protocol::Sftp)
    };
    dd_ftp_storage::SiteManager::save_path(
        &dir.path().join("sites.toml"),
        &dd_ftp_storage::SiteConfig {
            sites: vec![site],
            default_site: Some(0),
        },
    )
    .unwrap();
    std::fs::write(
        dir.path().join("dd_ftp.toml"),
        "auto_connect_default = true\n",
    )
    .unwrap();
    let mut vault = crate::vault::Vault::memory();
    vault
        .save("Desk", "ada", "files.example", 22, "s3cret-value")
        .unwrap();
    let shell = mount(
        cx,
        ShellOptions {
            load_app_config: true,
            load_user_sites: true,
            vault,
            backend: crate::session::Backend::fake(sample_tree(
                crate::session::FakeHostKey::Trusted,
            )),
            ..opts(dir.path())
        },
    );
    shell.update(cx, |shell, _cx| {
        let text = until(shell, "Connected via SFTP");
        assert!(text.contains("readme.txt"), "{text}");
        assert!(text.contains("pass:************"), "{text}");
        assert!(!text.contains("s3cret-value"), "{text}");
        let file = std::fs::read_to_string(dir.path().join("sites.toml")).unwrap();
        assert!(!file.contains("s3cret-value"), "{file}");
        assert!(!file.contains("password"), "{file}");
    });
}

fn trusted_tree() -> std::sync::Arc<std::sync::Mutex<crate::session::FakeTree>> {
    std::sync::Arc::new(std::sync::Mutex::new(crate::session::FakeTree::empty()))
}

fn connect_sftp(shell: &mut Shell, cx: &mut gpui::Context<Shell>) {
    shell.fill_quick_connect(&secret_form(Protocol::Sftp));
    shell.handle_key("c", cx);
    let text = until(shell, "Connected via SFTP");
    assert!(text.contains("Connected via SFTP"), "{text}");
    assert!(!text.contains("s3cret-value"), "{text}");
    assert!(!text.contains("password"), "{text}");
}

#[gpui::test]
fn upload_and_download_round_trip_keeps_the_bytes(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let payload = b"payload-bytes";
    std::fs::write(dir.path().join("payload.txt"), payload).unwrap();
    let tree = trusted_tree();
    let mut options = opts(dir.path());
    options.backend = crate::session::Backend::Fake(std::sync::Arc::clone(&tree));
    let shell = mount(cx, options);
    shell.update(cx, |shell, cx| {
        connect_sftp(shell, cx);
        shell.handle_key("down", cx);
        shell.handle_key("down", cx);
        shell.handle_key("u", cx);
        let text = until(shell, "upload complete");
        assert!(text.contains("queue:pending=0 active=0 failed=0"), "{text}");
        assert!(!text.contains("s3cret-value"), "{text}");
        assert_eq!(
            tree.lock().unwrap().bytes("/payload.txt"),
            Some(payload.as_slice())
        );
        let text = until(shell, "Remote cwd:");
        assert!(text.contains("payload.txt"), "{text}");

        std::fs::remove_file(dir.path().join("payload.txt")).unwrap();
        shell.handle_key("2", cx);
        shell.handle_key("down", cx);
        shell.handle_key("down", cx);
        shell.handle_key("d", cx);
        let text = until(shell, "download complete");
        assert_eq!(
            std::fs::read(dir.path().join("payload.txt")).unwrap(),
            payload
        );
        assert!(!text.contains("s3cret-value"), "{text}");
        assert!(!text.contains("password"), "{text}");
    });
}

#[gpui::test]
fn overwrite_skip_leaves_the_remote_bytes(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("payload.txt"), b"local-bytes").unwrap();
    let tree = trusted_tree();
    tree.lock()
        .unwrap()
        .put_bytes("/payload.txt", b"remote-bytes");
    let mut options = opts(dir.path());
    options.backend = crate::session::Backend::Fake(std::sync::Arc::clone(&tree));
    let shell = mount(cx, options);
    shell.update(cx, |shell, cx| {
        connect_sftp(shell, cx);
        shell.handle_key("down", cx);
        shell.handle_key("down", cx);
        shell.handle_key("u", cx);
        let text = until(shell, "overwrite:payload.txt");
        assert!(text.contains("newer:"), "{text}");
        shell.handle_key("s", cx);
        let text = shell.visible_text();
        assert!(text.contains("Skipped payload.txt"), "{text}");
        assert!(!text.contains("overwrite:payload.txt"), "{text}");
        assert_eq!(
            tree.lock().unwrap().bytes("/payload.txt"),
            Some(b"remote-bytes".as_slice())
        );
        assert!(!text.contains("s3cret-value"), "{text}");
    });
}

#[gpui::test]
fn ctrl_c_cancels_the_active_transfer_and_one_worker_stays_capped(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("box");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.txt"), b"aaa").unwrap();
    std::fs::write(folder.join("b.txt"), b"bbb").unwrap();
    std::fs::write(
        dir.path().join("dd_ftp.toml"),
        "worker_max_concurrency = 1\n",
    )
    .unwrap();
    let tree = trusted_tree();
    tree.lock().unwrap().hold_transfers = true;
    let mut options = opts(dir.path());
    options.load_app_config = true;
    options.backend = crate::session::Backend::Fake(std::sync::Arc::clone(&tree));
    let shell = mount(cx, options);
    shell.update(cx, |shell, cx| {
        connect_sftp(shell, cx);
        shell.handle_key("down", cx);
        shell.handle_key("down", cx);
        shell.handle_key("u", cx);
        let text = shell.visible_text();
        assert!(text.contains("queue:pending=1 active=1"), "{text}");
        assert!(text.contains("workers=1"), "{text}");
        shell.handle_key("ctrl-c", cx);
        let text = until(shell, "Cancelled by user");
        assert!(text.contains("active=0"), "{text}");
        assert!(tree.lock().unwrap().files.is_empty(), "{text}");
        assert!(!text.contains("password"), "{text}");
    });
}

#[gpui::test]
fn move_uploads_then_deletes_the_local_file(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("payload.txt"), b"moved").unwrap();
    let tree = trusted_tree();
    let mut options = opts(dir.path());
    options.backend = crate::session::Backend::Fake(std::sync::Arc::clone(&tree));
    let shell = mount(cx, options);
    shell.update(cx, |shell, cx| {
        connect_sftp(shell, cx);
        shell.handle_key("down", cx);
        shell.handle_key("down", cx);
        shell.handle_key("shift-m", cx);
        let text = until(shell, "upload complete");
        assert_eq!(
            tree.lock().unwrap().bytes("/payload.txt"),
            Some(b"moved".as_slice())
        );
        assert!(!dir.path().join("payload.txt").exists(), "{text}");
        assert!(!text.contains("s3cret-value"), "{text}");
    });
}

#[test]
fn help_flag_text_names_the_window() {
    let help = include_str!("main.rs");
    assert!(help.contains("dd_ftp-gui [--help]"));
    assert!(help.contains("F3"));
}
