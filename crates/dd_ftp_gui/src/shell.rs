//! One window: local files, bookmarks, quick connect, and the Settings sheet.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant, SystemTime};

use dd_ftp_app::{
    clamp_worker_concurrency, OverwritePolicy, SettingsField, TextField,
    WORKER_MAX_CONCURRENCY_LIMIT,
};
use dd_ftp_core::{ConnectionInfo, Protocol};
use dd_ftp_storage::{AppConfig, SiteManager};
use gpui::{
    prelude::*, px, App, ClickEvent, Context, FocusHandle, Focusable, FontWeight, IntoElement,
    KeyDownEvent, ParentElement, Render, ScrollHandle, Styled, Window, div,
};
use ldnddev_theme::{
    ColorField, EditorKey, EditorOutcome, Palette, PickerOutcome, PickerRow, ThemeEditor,
    ThemePicker, ThemeSaveTarget, EXTRA_MODAL_HEADER, EXTRA_SELECTION, Rgb,
};

use crate::listing::{self, LocalEntry};
use crate::prefs::{self, GuiSettings};
use crate::shortcuts;
use crate::theme_load::{self, LoadedTheme};

const THEME_EXTRAS: &[ColorField] = &[EXTRA_MODAL_HEADER, EXTRA_SELECTION];
const PAGE_NAMES: [&str; 4] = ["Themes", "Window", "Session", "Help"];
const BUSY_BARS: [char; 4] = ['▃', '▅', '█', '▅'];
const BUSY_FRAME_MS: u128 = 150;
const CHROME_PX: f32 = 28.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Themes,
    Window,
    Session,
    Help,
}

impl Page {
    fn index(self) -> usize {
        match self {
            Self::Themes => 0,
            Self::Window => 1,
            Self::Session => 2,
            Self::Help => 3,
        }
    }

    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Window,
            2 => Self::Session,
            3 => Self::Help,
            _ => Self::Themes,
        }
    }

    fn name(self) -> &'static str {
        PAGE_NAMES[self.index()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pane {
    Local,
    Bookmarks,
    Host,
    Port,
    User,
    Protocol,
}

impl Pane {
    fn cycle(self, dir: i32) -> Self {
        let order = [
            Self::Local,
            Self::Bookmarks,
            Self::Host,
            Self::Port,
            Self::User,
            Self::Protocol,
        ];
        let index = order.iter().position(|pane| *pane == self).unwrap_or(0);
        let len = order.len() as i32;
        let next = (index as i32 + dir).rem_euclid(len) as usize;
        order[next]
    }
}

struct Notice {
    message: String,
    at: Instant,
}

pub struct ShellOptions {
    pub local_cwd: PathBuf,
    pub gui_path: PathBuf,
    pub app_config_path: PathBuf,
    pub load_app_config: bool,
    pub sites: Vec<ConnectionInfo>,
    pub load_user_sites: bool,
    pub project_root: PathBuf,
    pub home: Option<PathBuf>,
    pub config_home: Option<PathBuf>,
    pub omarchy: ldnddev_theme::OmarchyPaths,
    pub tick: bool,
    pub quit_runtime: bool,
}

impl ShellOptions {
    pub fn production() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let home = theme_load::home_dir();
        let app_config_path = AppConfig::default_path();
        let mut local_cwd = cwd.clone();
        if let Ok(cfg) = AppConfig::load_or_default()
            && let Some(path) = cfg.last_local_cwd.as_deref()
        {
            let candidate = PathBuf::from(path);
            if candidate.is_dir() {
                local_cwd = candidate;
            }
        }
        Self {
            local_cwd,
            gui_path: prefs::settings_path(),
            app_config_path,
            load_app_config: true,
            sites: Vec::new(),
            load_user_sites: true,
            project_root: cwd,
            home,
            config_home: ldnddev_theme::default_config_home(),
            omarchy: ldnddev_theme::OmarchyPaths::system(),
            tick: true,
            quit_runtime: true,
        }
    }
}

pub struct Shell {
    focus: FocusHandle,
    options_tick: bool,
    quit_runtime: bool,
    pub quit: bool,
    confirm_quit: bool,
    pub transfer_active: bool,
    connect_busy: bool,
    local_cwd: PathBuf,
    entries: Vec<LocalEntry>,
    local_index: usize,
    local_scroll: ScrollHandle,
    sites: Vec<ConnectionInfo>,
    bookmark_index: usize,
    pane: Pane,
    host: TextField,
    port: TextField,
    user: TextField,
    protocol: Protocol,
    toasts: Vec<Notice>,
    connect_rx: Option<Receiver<String>>,
    busy_armed: bool,
    follow_armed: bool,
    project_root: PathBuf,
    home: Option<PathBuf>,
    config_home: Option<PathBuf>,
    omarchy: ldnddev_theme::OmarchyPaths,
    palette: Palette,
    theme_snapshot: Option<Palette>,
    theme_label: String,
    theme_warning: Option<String>,
    follow_stamp: Option<String>,
    gui_path: PathBuf,
    gui: GuiSettings,
    app_config_path: PathBuf,
    settings_open: bool,
    page: Page,
    query: String,
    query_on: bool,
    picker: Option<ThemePicker>,
    editor: Option<ThemeEditor>,
    direct_editor: bool,
    settings_scroll: ScrollHandle,
    window_row: usize,
    session_field: SettingsField,
    session_editor: TextField,
    session_workers: usize,
    session_hide_dotfiles: bool,
    session_overwrite: OverwritePolicy,
    session_auto_connect: bool,
    session_last_local: TextField,
    hide_dotfiles: bool,
    help_scroll: usize,
    test_clipboard: String,
}

impl Shell {
    pub fn open(opts: ShellOptions, cx: &mut Context<Self>) -> Self {
        let gui = prefs::load_from(&opts.gui_path);
        let loaded = theme_load::load_theme(&opts.project_root, opts.home.as_deref(), &opts.omarchy);
        let mut shell = Self {
            focus: cx.focus_handle(),
            options_tick: opts.tick,
            quit_runtime: opts.quit_runtime,
            quit: false,
            confirm_quit: false,
            transfer_active: false,
            connect_busy: false,
            local_cwd: opts.local_cwd.clone(),
            entries: Vec::new(),
            local_index: 0,
            local_scroll: ScrollHandle::new(),
            sites: opts.sites,
            bookmark_index: 0,
            pane: Pane::Local,
            host: TextField::from_str(""),
            port: TextField::from_str("22"),
            user: TextField::from_str(""),
            protocol: Protocol::Sftp,
            toasts: Vec::new(),
            connect_rx: None,
            busy_armed: false,
            follow_armed: false,
            project_root: opts.project_root,
            home: opts.home,
            config_home: opts.config_home,
            omarchy: opts.omarchy,
            palette: loaded.palette,
            theme_snapshot: None,
            theme_label: loaded.label,
            theme_warning: loaded.warning,
            follow_stamp: None,
            gui_path: opts.gui_path,
            gui,
            app_config_path: opts.app_config_path,
            settings_open: false,
            page: Page::Themes,
            query: String::new(),
            query_on: false,
            picker: None,
            editor: None,
            direct_editor: false,
            settings_scroll: ScrollHandle::new(),
            window_row: 0,
            session_field: SettingsField::Editor,
            session_editor: TextField::from_str(""),
            session_workers: 2,
            session_hide_dotfiles: false,
            session_overwrite: OverwritePolicy::Ask,
            session_auto_connect: false,
            session_last_local: TextField::from_str(&opts.local_cwd.display().to_string()),
            hide_dotfiles: false,
            help_scroll: 0,
            test_clipboard: String::new(),
        };
        shell.follow_stamp = ldnddev_theme::follow_stamp(&shell.omarchy);
        if opts.load_app_config {
            shell.load_app_config();
        }
        if opts.load_user_sites {
            shell.load_sites();
        }
        shell.session_last_local = TextField::from_str(&shell.local_cwd.display().to_string());
        shell.reload_local();
        if opts.tick {
            shell.arm_follow(cx);
        }
        shell
    }

    fn load_app_config(&mut self) {
        let Ok(text) = std::fs::read_to_string(&self.app_config_path) else {
            return;
        };
        let Ok(cfg) = AppConfig::from_toml(&text) else {
            self.push_toast("Could not read dd_ftp.toml".to_string());
            return;
        };
        self.session_editor = TextField::from_str(&cfg.editor_or_empty());
        self.session_workers = cfg.worker_max_or_default();
        self.session_hide_dotfiles = cfg.hide_dotfiles;
        self.hide_dotfiles = cfg.hide_dotfiles;
        self.session_overwrite = OverwritePolicy::from_config(cfg.overwrite_policy_or_default());
        self.session_auto_connect = cfg.auto_connect_default;
        if let Some(path) = cfg.last_local_cwd.as_deref() {
            let candidate = PathBuf::from(path);
            if candidate.is_dir() {
                self.local_cwd = candidate;
            }
        }
    }

    fn load_sites(&mut self) {
        match SiteManager::load_or_default() {
            Ok(cfg) => self.sites = cfg.sites,
            Err(err) => self.push_toast(format!("Bookmarks were not read ({err})")),
        }
    }

    #[cfg(test)]
    pub fn set_transfer_active(&mut self, active: bool) {
        self.transfer_active = active;
    }

    #[allow(dead_code)]
    pub fn set_test_clipboard(&mut self, text: impl Into<String>) {
        self.test_clipboard = text.into();
    }

    #[cfg(test)]
    pub fn palette_digest(&self) -> String {
        self.palette
            .colors()
            .iter()
            .map(|(key, color)| format!("{key}={}", color.to_hex()))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn reload_local(&mut self) {
        self.entries = listing::list_dir(&self.local_cwd, self.hide_dotfiles);
        if self.local_index >= self.entries.len() {
            self.local_index = self.entries.len().saturating_sub(1);
        }
    }

    fn push_toast(&mut self, message: String) {
        self.toasts.push(Notice {
            message,
            at: Instant::now(),
        });
        if self.toasts.len() > 4 {
            let extra = self.toasts.len() - 4;
            self.toasts.drain(0..extra);
        }
    }

    pub fn poll_jobs(&mut self) {
        let now = Instant::now();
        self.toasts
            .retain(|notice| now.duration_since(notice.at) < Duration::from_secs(5));
        if let Some(rx) = &self.connect_rx {
            match rx.try_recv() {
                Ok(message) => {
                    self.push_toast(message);
                    self.connect_rx = None;
                    self.connect_busy = false;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.connect_rx = None;
                    self.connect_busy = false;
                }
            }
        }
    }

    fn header_busy(&self) -> bool {
        self.connect_busy || self.transfer_active
    }

    pub fn busy_label(now_ms: u128) -> String {
        let bar = BUSY_BARS[(now_ms / BUSY_FRAME_MS) as usize % BUSY_BARS.len()];
        format!("{bar} d_d {bar}")
    }

    fn now_ms() -> u128 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    }

    fn arm_busy(&mut self, cx: &mut Context<Self>) {
        if !self.options_tick || self.busy_armed || !self.header_busy() {
            return;
        }
        self.busy_armed = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                let Ok(keep) = this.update(cx, |shell, cx| {
                    shell.poll_jobs();
                    let keep = shell.header_busy();
                    if !keep {
                        shell.busy_armed = false;
                    }
                    cx.notify();
                    keep
                }) else {
                    break;
                };
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    fn arm_follow(&mut self, cx: &mut Context<Self>) {
        if !self.options_tick || self.follow_armed {
            return;
        }
        self.follow_armed = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_secs(2))
                .await;
            if this
                .update(cx, |shell, cx| {
                    shell.refresh_follow();
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
        })
        .detach();
    }

    fn refresh_follow(&mut self) {
        if self.settings_open {
            return;
        }
        if !matches!(self.palette.omarchy, Some(ldnddev_theme::OmarchyChoice::Follow)) {
            return;
        }
        let stamp = ldnddev_theme::follow_stamp(&self.omarchy);
        if stamp == self.follow_stamp {
            return;
        }
        self.apply_loaded(theme_load::load_theme(
            &self.project_root,
            self.home.as_deref(),
            &self.omarchy,
        ));
        self.follow_stamp = stamp;
    }

    fn apply_loaded(&mut self, loaded: LoadedTheme) {
        self.palette = loaded.palette;
        self.theme_label = loaded.label;
        self.theme_warning = loaded.warning;
    }

    pub fn connect(&mut self, cx: &mut Context<Self>) {
        let host = self.host.value.trim().to_string();
        if host.is_empty() {
            self.push_toast("Connect needs a host".to_string());
            return;
        }
        let port = self.port.value.trim().to_string();
        let user = self.user.value.trim().to_string();
        let protocol = protocol_label(self.protocol);
        let (tx, rx) = mpsc::channel();
        self.connect_rx = Some(rx);
        self.connect_busy = true;
        std::thread::spawn(move || {
            // A live FTP/SFTP handshake pulls the protocol crates into this
            // window. Report the host the button would use and return.
            let who = if user.is_empty() {
                host.clone()
            } else {
                format!("{user}@{host}")
            };
            let _ = tx.send(format!("Connect {who}:{port} ({protocol})"));
        });
        self.arm_busy(cx);
    }

    fn request_quit(&mut self) {
        if self.transfer_active {
            self.confirm_quit = true;
        } else {
            self.quit = true;
        }
    }

    fn footer_text(&self) -> String {
        if self.settings_open || self.confirm_quit {
            "F1: Settings  Esc: Close  Ctrl+Q: Quit".to_string()
        } else {
            "F1: Settings  Ctrl+Q: Quit".to_string()
        }
    }

    fn editing_hex(&self) -> bool {
        self.editor
            .as_ref()
            .is_some_and(|editor| editor.editing_hex)
    }

    fn body_text_focused(&self) -> bool {
        matches!(self.pane, Pane::Host | Pane::Port | Pane::User)
    }

    fn settings_text_focused(&self) -> bool {
        self.settings_open
            && self.page == Page::Session
            && self.session_field.is_text()
            && self.query.is_empty()
            && !self.query_on
    }

    fn text_field_mut(&mut self) -> Option<&mut TextField> {
        if self.settings_open {
            if self.query_on {
                return None;
            }
            return match self.session_field {
                SettingsField::Editor => Some(&mut self.session_editor),
                SettingsField::LastLocalCwd => Some(&mut self.session_last_local),
                _ => None,
            };
        }
        match self.pane {
            Pane::Host => Some(&mut self.host),
            Pane::Port => Some(&mut self.port),
            Pane::User => Some(&mut self.user),
            _ => None,
        }
    }

    pub fn handle_key(&mut self, key: &str, cx: &mut Context<Self>) {
        self.poll_jobs();
        let key = key.to_ascii_lowercase();
        if key == "ctrl-v" {
            let text = self.test_clipboard.clone();
            if !text.is_empty() {
                self.paste_text(&text);
            }
            return;
        }
        if self.confirm_quit {
            match key.as_str() {
                "enter" | "y" => self.quit = true,
                "escape" | "n" => self.confirm_quit = false,
                _ => {}
            }
            return;
        }
        if key == "ctrl-q" {
            self.request_quit();
            return;
        }
        if key == "q" && !self.settings_open && !self.body_text_focused() {
            return;
        }
        if self.settings_open {
            self.settings_key(&key);
            return;
        }
        if is_settings_toggle(&key) {
            self.open_settings(Page::Themes);
            return;
        }
        if key == "f3" {
            self.open_settings(Page::Session);
            return;
        }
        self.body_key(&key, cx);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if is_paste(event) {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.paste_text(&text);
            }
            cx.notify();
            return;
        }
        self.handle_key(&chord(event), cx);
        cx.notify();
    }

    fn paste_text(&mut self, text: &str) {
        if self.editing_hex() {
            if let Some(editor) = self.editor.as_mut() {
                for ch in text.chars() {
                    if ch.is_ascii_hexdigit() || ch == '#' {
                        let _ = editor.handle(EditorKey::Char(ch), false);
                    }
                }
            }
            return;
        }
        if self.query_on {
            self.query.push_str(&text.replace(['\n', '\r'], ""));
            return;
        }
        let Some(field) = self.text_field_mut() else {
            return;
        };
        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                continue;
            }
            field.insert_char(ch);
        }
    }

    fn body_key(&mut self, key: &str, cx: &mut Context<Self>) {
        if key == "tab" {
            self.pane = self.pane.cycle(1);
            return;
        }
        if key == "shift-tab" {
            self.pane = self.pane.cycle(-1);
            return;
        }
        match self.pane {
            Pane::Local => self.local_key(key),
            Pane::Bookmarks => self.bookmark_key(key),
            Pane::Host | Pane::Port | Pane::User => self.form_key(key, cx),
            Pane::Protocol => self.protocol_key(key, cx),
        }
    }

    fn local_key(&mut self, key: &str) {
        let len = self.entries.len();
        if len == 0 {
            return;
        }
        let vim = self.gui.vim_keys;
        match key {
            "up" | "k" if key != "k" || vim => {
                self.local_index = self.local_index.saturating_sub(1);
            }
            "down" | "j" if key != "j" || vim => {
                self.local_index = (self.local_index + 1).min(len - 1);
            }
            "g" if vim => self.local_index = 0,
            "shift-g" if vim => self.local_index = len - 1,
            "pageup" | "page-up" => {
                self.local_index = self.local_index.saturating_sub(10);
            }
            "pagedown" | "page-down" => {
                self.local_index = (self.local_index + 10).min(len - 1);
            }
            "enter" => self.enter_local(),
            _ => {}
        }
    }

    fn enter_local(&mut self) {
        let Some(entry) = self.entries.get(self.local_index) else {
            return;
        };
        if !entry.is_dir() {
            return;
        }
        let next = entry.path.clone();
        if next.is_dir() {
            self.local_cwd = next;
            self.local_index = 0;
            self.reload_local();
        }
    }

    fn bookmark_key(&mut self, key: &str) {
        let len = self.sites.len();
        if len == 0 {
            return;
        }
        let vim = self.gui.vim_keys;
        match key {
            "up" | "k" if key != "k" || vim => {
                self.bookmark_index = self.bookmark_index.saturating_sub(1);
            }
            "down" | "j" if key != "j" || vim => {
                self.bookmark_index = (self.bookmark_index + 1).min(len - 1);
            }
            "enter" => self.fill_from_bookmark(),
            _ => {}
        }
    }

    fn fill_from_bookmark(&mut self) {
        let Some(site) = self.sites.get(self.bookmark_index) else {
            return;
        };
        self.host = TextField::from_str(&site.host);
        self.port = TextField::from_str(&site.port.to_string());
        self.user = TextField::from_str(&site.username);
        self.protocol = site.protocol;
        self.push_toast(format!("Bookmark {}", site.host));
    }

    fn form_key(&mut self, key: &str, cx: &mut Context<Self>) {
        if key == "enter" {
            self.connect(cx);
            return;
        }
        let Some(field) = self.text_field_mut() else {
            return;
        };
        match key {
            "backspace" => field.backspace(),
            "delete" => field.delete(),
            "left" => field.move_cursor(-1, false),
            "right" => field.move_cursor(1, false),
            "home" => field.move_home(false),
            "end" => field.move_end(false),
            "up" => field.move_home(false),
            "down" => field.move_end(false),
            _ => {
                if let Some(ch) = typed_char(key) {
                    field.insert_char(ch);
                }
            }
        }
    }

    fn protocol_key(&mut self, key: &str, cx: &mut Context<Self>) {
        match key {
            "left" => self.cycle_protocol(-1),
            "right" | "space" => self.cycle_protocol(1),
            "enter" => self.connect(cx),
            _ => {}
        }
    }

    fn cycle_protocol(&mut self, dir: i32) {
        let current_default = default_port(self.protocol);
        self.protocol = if dir < 0 {
            match self.protocol {
                Protocol::Sftp => Protocol::Ftps,
                Protocol::Ftp => Protocol::Sftp,
                Protocol::Ftps => Protocol::Ftp,
            }
        } else {
            match self.protocol {
                Protocol::Sftp => Protocol::Ftp,
                Protocol::Ftp => Protocol::Ftps,
                Protocol::Ftps => Protocol::Sftp,
            }
        };
        if self.port.value.trim().parse::<u16>().ok() == Some(current_default)
            || self.port.value.trim().is_empty()
        {
            self.port = TextField::from_str(&default_port(self.protocol).to_string());
        }
    }

    fn open_settings(&mut self, page: Page) {
        if self.settings_open {
            if page == Page::Session {
                self.page = Page::Session;
                self.session_field = SettingsField::Editor;
                self.help_scroll = 0;
            }
            return;
        }
        self.theme_snapshot = Some(self.palette.clone());
        self.settings_open = true;
        self.page = page;
        self.query.clear();
        self.query_on = false;
        self.help_scroll = 0;
        self.hydrate_session();
        self.build_theme_ui();
    }

    fn hydrate_session(&mut self) {
        self.session_field = SettingsField::Editor;
        self.session_last_local = TextField::from_str(&self.local_cwd.display().to_string());
        self.session_hide_dotfiles = self.hide_dotfiles;
    }

    fn build_theme_ui(&mut self) {
        let picker = ThemePicker::new(self.palette.clone(), THEME_EXTRAS, self.omarchy.clone());
        if picker.lists_omarchy() {
            self.picker = Some(picker);
            self.editor = None;
            self.direct_editor = false;
        } else {
            self.picker = None;
            self.direct_editor = true;
            self.editor = Some(ThemeEditor::new(self.palette.clone(), THEME_EXTRAS));
        }
    }

    fn close_settings(&mut self, revert: bool) {
        if !self.settings_open {
            return;
        }
        if revert {
            if let Some(snapshot) = self.theme_snapshot.take() {
                self.palette = snapshot;
            }
        } else {
            self.theme_snapshot = None;
        }
        self.settings_open = false;
        self.picker = None;
        self.editor = None;
        self.direct_editor = false;
        self.query.clear();
        self.query_on = false;
        self.page = Page::Themes;
        self.help_scroll = 0;
    }

    fn settings_key(&mut self, key: &str) {
        if is_settings_toggle(key) {
            self.close_settings(true);
            return;
        }
        if key == "f3" {
            self.page = Page::Session;
            self.session_field = SettingsField::Editor;
            self.query.clear();
            self.query_on = false;
            return;
        }
        if self.query_on || !self.query.is_empty() {
            self.search_key(key);
            return;
        }
        if key == "/" && !self.editing_hex() && !self.settings_text_focused() {
            self.query_on = true;
            return;
        }
        if self.try_page_key(key) {
            return;
        }
        match self.page {
            Page::Themes => self.themes_key(key),
            Page::Window => self.window_key(key),
            Page::Session => self.session_key(key),
            Page::Help => self.help_key(key),
        }
    }

    fn search_key(&mut self, key: &str) {
        match key {
            "escape" => {
                self.query.clear();
                self.query_on = false;
            }
            "backspace" => {
                self.query.pop();
            }
            "enter" => {
                if let Some(page) = self.result_page() {
                    self.query.clear();
                    self.query_on = false;
                    self.page = page;
                }
            }
            "up" | "down" => {}
            _ => {
                if let Some(ch) = typed_char(key) {
                    self.query_on = true;
                    self.query.push(ch);
                }
            }
        }
    }

    fn result_page(&self) -> Option<Page> {
        let query = self.query.to_lowercase();
        if query.is_empty() {
            return None;
        }
        for (label, page) in [
            ("themes", Page::Themes),
            ("density", Page::Window),
            ("vim", Page::Window),
            ("window", Page::Window),
            ("workers", Page::Session),
            ("editor", Page::Session),
            ("hide dotfiles", Page::Session),
            ("session", Page::Session),
            ("help", Page::Help),
            ("settings", Page::Help),
        ] {
            if label.contains(&query) || query.contains(label) {
                return Some(page);
            }
        }
        None
    }

    fn try_page_key(&mut self, key: &str) -> bool {
        if self.page == Page::Session && (key == "tab" || key == "shift-tab") {
            return false;
        }
        if self.editing_hex() && (key == "[" || key == "]") {
            return false;
        }
        if self.page == Page::Themes && self.editor.is_some() && (key == "[" || key == "]") {
            return false;
        }
        if self.page == Page::Session && self.session_field.is_text() && (key == "[" || key == "]")
        {
            return false;
        }
        let dir = match key {
            "tab" | "]" => 1,
            "shift-tab" | "[" => -1,
            _ => return false,
        };
        let next = self.page.index() as i32 + dir;
        let next = next.rem_euclid(PAGE_NAMES.len() as i32) as usize;
        self.page = Page::from_index(next);
        self.session_field = SettingsField::Editor;
        self.help_scroll = 0;
        true
    }

    fn themes_key(&mut self, key: &str) {
        if key == "t" && !self.editing_hex() {
            self.toggle_save_target();
            return;
        }
        if key == "escape" {
            self.theme_escape();
            return;
        }
        if self.editor.is_some() {
            self.editor_key(key);
        } else if self.picker.is_some() {
            self.picker_key(key);
        }
    }

    fn theme_escape(&mut self) {
        if self.editor.is_some() {
            if self.editing_hex() {
                self.editor_key("escape");
                return;
            }
            if self.picker.is_some() {
                if let Some(editor) = self.editor.as_mut() {
                    editor.revert();
                }
                let (palette, target) = self
                    .editor
                    .as_ref()
                    .map(|editor| (editor.palette.clone(), editor.save_target))
                    .unwrap();
                self.editor = None;
                if let Some(picker) = self.picker.as_mut() {
                    picker.palette = palette.clone();
                    picker.save_target = target;
                }
                self.palette = palette;
                return;
            }
            self.close_settings(true);
            return;
        }
        self.close_settings(true);
    }

    fn editor_key(&mut self, key: &str) {
        let Some(ek) = editor_key(key) else {
            return;
        };
        let outcome = {
            let Some(editor) = self.editor.as_mut() else {
                return;
            };
            editor.handle(ek, key.starts_with("shift-"))
        };
        match outcome {
            EditorOutcome::PaletteChanged => self.sync_editor_palette(),
            EditorOutcome::RequestSave => self.save_theme(),
            EditorOutcome::Closed { .. } => self.theme_escape_after_editor_closed(),
            EditorOutcome::HexError(msg) => self.push_toast(msg),
            EditorOutcome::None => {}
        }
    }

    fn theme_escape_after_editor_closed(&mut self) {
        if self.picker.is_some() {
            let (palette, target) = self
                .editor
                .as_ref()
                .map(|editor| (editor.palette.clone(), editor.save_target))
                .unwrap_or_else(|| (self.palette.clone(), ThemeSaveTarget::Global));
            self.editor = None;
            if let Some(picker) = self.picker.as_mut() {
                picker.palette = palette.clone();
                picker.save_target = target;
            }
            self.palette = palette;
        } else {
            self.close_settings(true);
        }
    }

    fn sync_editor_palette(&mut self) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        let palette = editor.palette.clone();
        let target = editor.save_target;
        if let Some(picker) = self.picker.as_mut() {
            picker.palette = palette.clone();
            picker.save_target = target;
        }
        self.palette = palette;
    }

    fn picker_key(&mut self, key: &str) {
        let moved = matches!(
            key,
            "up" | "down" | "j" | "k" | "g" | "shift-g" | "pageup" | "pagedown" | "page-up"
                | "page-down"
        );
        if (key == "j" || key == "k" || key == "g" || key == "shift-g") && !self.gui.vim_keys {
            return;
        }
        if key == "pageup" || key == "page-up" || key == "pagedown" || key == "page-down" {
            let step = if key.starts_with("pageu") || key == "page-up" {
                -8
            } else {
                8
            };
            if let Some(picker) = self.picker.as_mut() {
                let next = picker.selected as i32 + step;
                let last = picker.rows.len().saturating_sub(1) as i32;
                picker.select(next.clamp(0, last) as usize);
            }
            self.preview_theme();
            return;
        }
        let Some(ek) = editor_key(key) else {
            return;
        };
        let outcome = {
            let Some(picker) = self.picker.as_mut() else {
                return;
            };
            picker.handle(ek)
        };
        if moved {
            self.preview_theme();
            return;
        }
        match outcome {
            PickerOutcome::Applied => self.copy_picker_palette(),
            PickerOutcome::EditColors => self.open_color_editor(),
            PickerOutcome::RequestSave => self.save_theme(),
            PickerOutcome::Error(msg) => self.push_toast(msg),
            PickerOutcome::Closed { .. } => self.close_settings(true),
            PickerOutcome::None => {}
        }
    }

    fn preview_theme(&mut self) {
        let palette = {
            let Some(picker) = self.picker.as_mut() else {
                return;
            };
            let _ = picker.preview_selected();
            picker.palette.clone()
        };
        self.palette = palette;
    }

    fn copy_picker_palette(&mut self) {
        if let Some(picker) = self.picker.as_ref() {
            self.palette = picker.palette.clone();
        }
    }

    fn open_color_editor(&mut self) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        let mut editor = ThemeEditor::new(picker.palette.clone(), THEME_EXTRAS);
        editor.save_target = picker.save_target;
        self.editor = Some(editor);
    }

    fn toggle_save_target(&mut self) {
        if let Some(editor) = self.editor.as_mut() {
            editor.save_target = editor.save_target.toggled();
            let target = editor.save_target;
            if let Some(picker) = self.picker.as_mut() {
                picker.save_target = target;
            }
            return;
        }
        if let Some(picker) = self.picker.as_mut() {
            picker.save_target = picker.save_target.toggled();
        }
    }

    fn save_target(&self) -> ThemeSaveTarget {
        if let Some(editor) = &self.editor {
            editor.save_target
        } else if let Some(picker) = &self.picker {
            picker.save_target
        } else {
            ThemeSaveTarget::Global
        }
    }

    fn save_theme(&mut self) {
        let palette = if let Some(editor) = &self.editor {
            editor.palette.clone()
        } else if let Some(picker) = &self.picker {
            picker.palette.clone()
        } else {
            self.palette.clone()
        };
        let fields = self
            .editor
            .as_ref()
            .map(|editor| editor.fields.clone())
            .unwrap_or_else(|| {
                let mut fields = ldnddev_theme::COLOR_FIELDS.to_vec();
                fields.extend_from_slice(THEME_EXTRAS);
                fields
            });
        let target = self.save_target();
        match ldnddev_theme::save_theme(
            &palette,
            &self.project_root,
            theme_load::THEME_FILE,
            target,
            self.config_home.as_deref(),
            &fields,
        ) {
            Ok(path) => {
                self.palette = palette;
                self.theme_label = path.display().to_string();
                self.theme_warning = None;
                self.push_toast(format!("Theme saved to {}", path.display()));
            }
            Err(err) => self.push_toast(format!("Theme was not saved ({err})")),
        }
    }

    fn window_key(&mut self, key: &str) {
        let vim = self.gui.vim_keys;
        match key {
            "escape" => self.close_settings(true),
            "up" | "k" if key != "k" || vim => self.window_row = 0,
            "down" | "j" if key != "j" || vim => self.window_row = 1,
            "left" | "right" | "space" | "enter" => self.toggle_window_row(),
            _ => {}
        }
    }

    fn toggle_window_row(&mut self) {
        if self.window_row == 0 {
            self.gui.density = self.gui.density.toggled();
        } else {
            self.gui.vim_keys = !self.gui.vim_keys;
        }
        if let Err(err) = prefs::save_to(&self.gui_path, &self.gui) {
            self.push_toast(format!("gui.json was not saved ({err})"));
        }
    }

    fn session_key(&mut self, key: &str) {
        match key {
            "escape" => self.close_settings(true),
            "enter" => self.save_session(),
            "tab" | "down" => self.session_field = self.session_field.next(),
            "shift-tab" | "up" => self.session_field = self.session_field.prev(),
            "left" | "right" | "space" if !self.session_field.is_text() => {
                let dir = if key == "left" { -1 } else { 1 };
                self.cycle_session(dir);
            }
            _ if self.session_field.is_text() => self.session_text_key(key),
            _ => {}
        }
    }

    fn session_text_key(&mut self, key: &str) {
        let Some(field) = (match self.session_field {
            SettingsField::Editor => Some(&mut self.session_editor),
            SettingsField::LastLocalCwd => Some(&mut self.session_last_local),
            _ => None,
        }) else {
            return;
        };
        match key {
            "backspace" => field.backspace(),
            "delete" => field.delete(),
            "left" => field.move_cursor(-1, false),
            "right" => field.move_cursor(1, false),
            "home" => field.move_home(false),
            "end" => field.move_end(false),
            _ => {
                if let Some(ch) = typed_char(key) {
                    field.insert_char(ch);
                }
            }
        }
    }

    fn cycle_session(&mut self, dir: i32) {
        match self.session_field {
            SettingsField::Workers => {
                let n = self.session_workers as i32;
                let limit = WORKER_MAX_CONCURRENCY_LIMIT as i32;
                let next = if dir < 0 {
                    if n <= 1 { limit } else { n - 1 }
                } else if n >= limit {
                    1
                } else {
                    n + 1
                };
                self.session_workers = clamp_worker_concurrency(next as usize);
            }
            SettingsField::HideDotfiles => {
                self.session_hide_dotfiles = !self.session_hide_dotfiles;
            }
            SettingsField::Overwrite => {
                self.session_overwrite = if dir < 0 {
                    self.session_overwrite.settings_prev()
                } else {
                    self.session_overwrite.settings_next()
                };
            }
            SettingsField::AutoConnect => {
                self.session_auto_connect = !self.session_auto_connect;
            }
            SettingsField::Editor | SettingsField::LastLocalCwd => {}
        }
    }

    fn save_session(&mut self) {
        let editor = self.session_editor.value.trim().to_string();
        let last_local = self.session_last_local.value.trim().to_string();
        let overwrite = self.session_overwrite.to_config();
        let cfg = AppConfig {
            editor: if editor.is_empty() {
                None
            } else {
                Some(editor)
            },
            worker_max_concurrency: Some(self.session_workers as u32),
            last_local_cwd: if last_local.is_empty() {
                None
            } else {
                Some(last_local.clone())
            },
            hide_dotfiles: self.session_hide_dotfiles,
            overwrite_policy: if overwrite == "ask" {
                None
            } else {
                Some(overwrite.to_string())
            },
            auto_connect_default: self.session_auto_connect,
        };
        let Ok(text) = cfg.to_toml() else {
            self.push_toast("Settings were not saved".to_string());
            return;
        };
        if prefs::write_private(&self.app_config_path, &text).is_err() {
            self.push_toast(format!(
                "Settings were not saved to {}",
                self.app_config_path.display()
            ));
            return;
        }
        self.hide_dotfiles = self.session_hide_dotfiles;
        if !last_local.is_empty() {
            let path = PathBuf::from(&last_local);
            if path.is_dir() && path != self.local_cwd {
                self.local_cwd = path;
                self.local_index = 0;
            }
        }
        self.reload_local();
        self.push_toast(format!(
            "Settings saved to {}",
            self.app_config_path.display()
        ));
        self.close_settings(true);
    }

    fn help_key(&mut self, key: &str) {
        let vim = self.gui.vim_keys;
        match key {
            "escape" => self.close_settings(true),
            "down" | "j" if key != "j" || vim => {
                self.help_scroll = self.help_scroll.saturating_add(1);
            }
            "up" | "k" if key != "k" || vim => {
                self.help_scroll = self.help_scroll.saturating_sub(1);
            }
            "g" if vim => self.help_scroll = 0,
            _ => {}
        }
    }

    fn select_theme(&mut self, index: usize) {
        if let Some(picker) = self.picker.as_mut() {
            picker.select(index);
        }
        self.preview_theme();
    }

    #[cfg(test)]
    pub fn visible_text(&self) -> String {
        self.lines().join("\n")
    }

    #[cfg(test)]
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push("dd_ftp".to_string());
        if self.header_busy() {
            lines.push(Self::busy_label(Self::now_ms()));
        }
        lines.push(format!("footer:{}", self.footer_text()));
        lines.push(format!("local:{}", self.local_cwd.display()));
        for (index, entry) in self.entries.iter().enumerate() {
            let mark = if index == self.local_index && self.pane == Pane::Local {
                ">"
            } else {
                " "
            };
            lines.push(format!(
                "{mark} {}  {}  {}",
                entry.name,
                entry.size,
                entry.marker()
            ));
        }
        if self.sites.is_empty() {
            lines.push("No bookmarks".to_string());
        }
        for (index, site) in self.sites.iter().enumerate() {
            let mark = if index == self.bookmark_index { ">" } else { " " };
            lines.push(format!(
                "{mark} {}  {}@{}:{}  {}",
                site.name,
                site.username,
                site.host,
                site.port,
                protocol_label(site.protocol)
            ));
        }
        lines.push(format!("host:{}", self.host.value));
        lines.push(format!("port:{}", self.port.value));
        lines.push(format!("user:{}", self.user.value));
        lines.push(format!("protocol:{}", protocol_label(self.protocol)));
        for notice in &self.toasts {
            lines.push(format!("toast:{}", notice.message));
        }
        if self.confirm_quit {
            lines.push("Quit while a transfer is active?".to_string());
        }
        if self.settings_open {
            lines.push("settings-panel:right:70".to_string());
            lines.push("settings-nav:Themes,Window,Session,Help".to_string());
            let page = if !self.query.trim().is_empty() {
                "Results"
            } else {
                self.page.name()
            };
            lines.push(format!("settings-page:{page}"));
            lines.push("Settings".to_string());
            for (index, label) in PAGE_NAMES.iter().enumerate() {
                let mark = if self.query.trim().is_empty() && self.page.index() == index {
                    ">"
                } else {
                    " "
                };
                lines.push(format!("{mark} {label}"));
            }
            if !self.query.trim().is_empty() {
                lines.push("Results".to_string());
            } else {
                match self.page {
                    Page::Themes => self.push_theme_lines(&mut lines),
                    Page::Window => {
                        lines.push(format!("Density {}", self.gui.density.label()));
                        lines.push(format!(
                            "Vim keys {}",
                            if self.gui.vim_keys { "on" } else { "off" }
                        ));
                        lines.push("Comfortable".to_string());
                        lines.push("Compact".to_string());
                    }
                    Page::Session => {
                        lines.push(format!("Editor {}", self.session_editor.value));
                        lines.push(format!("Workers {}", self.session_workers));
                        lines.push(format!(
                            "Hide dotfiles {}",
                            if self.session_hide_dotfiles { "on" } else { "off" }
                        ));
                        lines.push(format!(
                            "Overwrite {}",
                            self.session_overwrite.settings_label()
                        ));
                        lines.push(format!(
                            "Auto-connect {}",
                            if self.session_auto_connect { "on" } else { "off" }
                        ));
                        lines.push(format!("Last local path {}", self.session_last_local.value));
                    }
                    Page::Help => lines.extend(shortcuts::lines()),
                }
            }
        }
        lines
    }

    #[cfg(test)]
    fn push_theme_lines(&self, lines: &mut Vec<String>) {
        lines.push(format!("Source: {}", self.theme_label));
        let status = self
            .theme_warning
            .clone()
            .unwrap_or_else(|| "OK (loaded cleanly)".to_string());
        lines.push(format!("Status: {status}"));
        let word = match self.save_target() {
            ThemeSaveTarget::Global => "Global",
            ThemeSaveTarget::Local => "Local",
        };
        lines.push(format!("Save to {word}"));
        if let Some(editor) = &self.editor {
            lines.push("Theme color editor".to_string());
            lines.push(if self.direct_editor { "Revert" } else { "Back" }.to_string());
            for (index, field) in editor.fields.iter().enumerate() {
                let hex = editor
                    .palette
                    .get(field.key)
                    .map(|color| color.to_hex())
                    .unwrap_or_else(|| "#000000".to_string());
                let mark = if index == editor.selected { ">" } else { " " };
                lines.push(format!("{mark} {} {hex}", field.key));
            }
            lines.push(format!("Hex: {}", editor.hex_draft));
        } else if let Some(picker) = &self.picker {
            lines.push("theme-list:flow".to_string());
            for (index, row) in picker.rows.iter().enumerate() {
                let mark = if index == picker.selected { ">" } else { " " };
                lines.push(format!("{mark} {}", theme_row_label(row)));
            }
            lines.push("Edit colors…".to_string());
        }
        lines.push("Save".to_string());
    }

    fn row_h(&self) -> f32 {
        self.gui.density.row()
    }
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.poll_jobs();
        if self.quit && self.quit_runtime {
            cx.quit();
        }
        let paint = paint_of(&self.palette);
        let row = self.row_h();
        let footer = self.footer_text();
        let busy = self.header_busy().then(|| Self::busy_label(Self::now_ms()));
        let entries: Vec<(usize, String, bool, bool)> = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                (
                    index,
                    format!("{}  {}  {}", entry.name, entry.size, entry.marker()),
                    entry.is_dir(),
                    index == self.local_index,
                )
            })
            .collect();
        let sites: Vec<(usize, String, bool)> = self
            .sites
            .iter()
            .enumerate()
            .map(|(index, site)| {
                (
                    index,
                    format!(
                        "{}  {}@{}:{}  {}",
                        site.name,
                        site.username,
                        site.host,
                        site.port,
                        protocol_label(site.protocol)
                    ),
                    index == self.bookmark_index,
                )
            })
            .collect();
        let host = self.host.value.clone();
        let port = self.port.value.clone();
        let user = self.user.value.clone();
        let protocol = protocol_label(self.protocol).to_string();
        let cwd = self.local_cwd.display().to_string();
        let toasts: Vec<String> = self.toasts.iter().map(|notice| notice.message.clone()).collect();
        let confirm = self.confirm_quit;
        let settings = self.settings_open.then(|| self.settings_view());
        let local_scroll = self.local_scroll.clone();
        let pane = self.pane;

        let mut list = div()
            .id("local-list")
            .absolute()
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&local_scroll)
            .flex()
            .flex_col();
        for (index, label, is_dir, on) in entries {
            list = list.child(
                div()
                    .id(("local-row", index))
                    .h(px(row))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .when(on && pane == Pane::Local, |row| row.bg(paint.selected))
                    .on_click(cx.listener(move |shell, _: &ClickEvent, _, cx| {
                        let enter = shell.pane == Pane::Local && shell.local_index == index;
                        shell.pane = Pane::Local;
                        shell.local_index = index;
                        if enter && is_dir {
                            shell.enter_local();
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .child(label),
            );
        }

        let mut bookmarks = div().flex().flex_col();
        if sites.is_empty() {
            bookmarks = bookmarks.child(
                div()
                    .h(px(row))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .text_color(paint.secondary)
                    .child("No bookmarks"),
            );
        }
        for (index, label, on) in sites {
            bookmarks = bookmarks.child(
                div()
                    .id(("bookmark", index))
                    .h(px(row))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .when(on, |row| row.bg(paint.selected))
                    .on_click(cx.listener(move |shell, _: &ClickEvent, _, cx| {
                        shell.pane = Pane::Bookmarks;
                        shell.bookmark_index = index;
                        shell.fill_from_bookmark();
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .child(label),
            );
        }

        let mut column = div()
            .id("shell")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, _, cx| {
                shell.on_key_down(event, cx);
            }))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(paint.base)
            .text_color(paint.text)
            .text_size(px(13.0))
            .child(header_bar(&cwd, busy.as_deref(), paint))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .flex_row()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .h_full()
                            .relative()
                            .border_r_1()
                            .border_color(paint.border)
                            .child(list),
                    )
                    .child(
                        div()
                            .w(px(360.0))
                            .h_full()
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .bg(paint.panel)
                            .child(
                                div()
                                    .h(px(row))
                                    .px(px(8.0))
                                    .flex()
                                    .items_center()
                                    .font_weight(FontWeight::BOLD)
                                    .child("Bookmarks"),
                            )
                            .child(
                                    div()
                                        .id("bookmarks")
                                        .flex_1()
                                        .min_h(px(0.0))
                                        .overflow_y_scroll()
                                        .child(bookmarks),
                                )
                            .child(connect_form(
                                cx,
                                &ConnectFields {
                                    host: &host,
                                    port: &port,
                                    user: &user,
                                    protocol: &protocol,
                                },
                                row,
                                paint,
                                pane,
                            )),
                    ),
            );
        for (index, message) in toasts.iter().enumerate() {
            column = column.child(
                div()
                    .absolute()
                    .bottom(px(row + 8.0 + (index as f32 * 28.0)))
                    .right(px(12.0))
                    .px(px(8.0))
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .bg(paint.raised)
                    .border_1()
                    .border_color(paint.border)
                    .child(message.clone()),
            );
        }
        if confirm {
            column = column.child(confirm_box(cx, paint));
        }
        if let Some(settings) = settings {
            column = column.child(settings_panel(cx, &settings, row, paint, &self.settings_scroll));
        }
        column.child(
            div()
                .h(px(CHROME_PX))
                .px(px(8.0))
                .flex()
                .items_center()
                .border_t_1()
                .border_color(paint.border)
                .bg(paint.panel)
                .text_color(paint.secondary)
                .child(footer),
        )
    }
}

struct SettingsView {
    page_name: String,
    nav_on: Option<usize>,
    hint: String,
    theme_rows: Vec<(usize, String, bool)>,
    editor: bool,
    direct_editor: bool,
    editor_rows: Vec<(usize, String, bool)>,
    hex: String,
    source: String,
    save_to: String,
    density: String,
    vim: String,
    window_row: usize,
    session: Vec<String>,
    help: Vec<String>,
    query: String,
}

impl Shell {
    fn settings_view(&self) -> SettingsView {
        let page_name = if !self.query.trim().is_empty() {
            "Results".to_string()
        } else {
            self.page.name().to_string()
        };
        let mut theme_rows = Vec::new();
        let mut editor_rows = Vec::new();
        let mut hex = String::new();
        if let Some(editor) = &self.editor {
            for (index, field) in editor.fields.iter().enumerate() {
                let color = editor
                    .palette
                    .get(field.key)
                    .map(|color| color.to_hex())
                    .unwrap_or_else(|| "#000000".to_string());
                editor_rows.push((
                    index,
                    format!("{} {color}", field.key),
                    index == editor.selected,
                ));
            }
            hex = editor.hex_draft.clone();
        } else if let Some(picker) = &self.picker {
            for (index, row) in picker.rows.iter().enumerate() {
                theme_rows.push((
                    index,
                    theme_row_label(row),
                    index == picker.selected,
                ));
            }
        }
        let word = match self.save_target() {
            ThemeSaveTarget::Global => "Global",
            ThemeSaveTarget::Local => "Local",
        };
        SettingsView {
            page_name,
            nav_on: self.query.trim().is_empty().then_some(self.page.index()),
            hint: "j/k move   Enter apply   e colors   t Global/Local   y/s save   Esc revert"
                .to_string(),
            theme_rows,
            editor: self.editor.is_some(),
            direct_editor: self.direct_editor,
            editor_rows,
            hex,
            source: self.theme_label.clone(),
            save_to: format!("Save to {word}"),
            density: format!("Density {}", self.gui.density.label()),
            vim: format!(
                "Vim keys {}",
                if self.gui.vim_keys { "on" } else { "off" }
            ),
            window_row: self.window_row,
            session: vec![
                format!("Editor {}", self.session_editor.value),
                format!("Workers {}", self.session_workers),
                format!(
                    "Hide dotfiles {}",
                    if self.session_hide_dotfiles { "on" } else { "off" }
                ),
                format!("Overwrite {}", self.session_overwrite.settings_label()),
                format!(
                    "Auto-connect {}",
                    if self.session_auto_connect { "on" } else { "off" }
                ),
                format!("Last local path {}", self.session_last_local.value),
            ],
            help: shortcuts::lines(),
            query: self.query.clone(),
        }
    }
}

#[derive(Clone, Copy)]
struct Paint {
    base: gpui::Hsla,
    panel: gpui::Hsla,
    text: gpui::Hsla,
    secondary: gpui::Hsla,
    border: gpui::Hsla,
    selected: gpui::Hsla,
    raised: gpui::Hsla,
}

fn paint_of(palette: &Palette) -> Paint {
    Paint {
        base: hsla_key(palette, "base_background", Rgb::new(0x11, 0x11, 0x18)),
        panel: hsla_key(palette, "body_background", Rgb::new(0x16, 0x16, 0x20)),
        text: hsla_key(palette, "text_primary", Rgb::new(0xF5, 0xF6, 0xF7)),
        secondary: hsla_key(palette, "text_secondary", Rgb::new(0xB0, 0xB4, 0xBA)),
        border: hsla_key(palette, "border_default", Rgb::new(0x3A, 0x3A, 0x44)),
        selected: hsla_key(palette, "selected_background", Rgb::new(0x20, 0x60, 0xA0)),
        raised: hsla_key(palette, "modal_background", Rgb::new(0x1C, 0x1C, 0x28)),
    }
}

fn hsla_key(palette: &Palette, key: &str, fallback: Rgb) -> gpui::Hsla {
    hsla(palette.get(key).unwrap_or(fallback))
}

fn hsla(color: Rgb) -> gpui::Hsla {
    gpui::Rgba {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: 1.0,
    }
    .into()
}

fn header_bar(cwd: &str, busy: Option<&str>, paint: Paint) -> gpui::Div {
    let mut bar = div()
        .h(px(CHROME_PX))
        .px(px(8.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .bg(paint.raised)
        .border_b_1()
        .border_color(paint.border)
        .child(
            div()
                .font_weight(FontWeight::BOLD)
                .child("dd_ftp"),
        );
    if let Some(busy) = busy {
        bar = bar.child(busy.to_string());
    }
    bar.child(div().flex_1()).child(
        div()
            .text_color(paint.secondary)
            .overflow_hidden()
            .whitespace_nowrap()
            .child(cwd.to_string()),
    )
}

struct ConnectFields<'a> {
    host: &'a str,
    port: &'a str,
    user: &'a str,
    protocol: &'a str,
}

fn connect_form(
    cx: &mut Context<Shell>,
    fields: &ConnectFields<'_>,
    row: f32,
    paint: Paint,
    pane: Pane,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .p(px(8.0))
        .border_t_1()
        .border_color(paint.border)
        .child(field_row(cx, "Host", fields.host, Pane::Host, pane, row, paint))
        .child(field_row(cx, "Port", fields.port, Pane::Port, pane, row, paint))
        .child(field_row(cx, "User", fields.user, Pane::User, pane, row, paint))
        .child(field_row(
            cx,
            "Protocol",
            fields.protocol,
            Pane::Protocol,
            pane,
            row,
            paint,
        ))
        .child(
            div()
                .id("connect-button")
                .h(px(row))
                .px(px(8.0))
                .mt(px(4.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .bg(paint.selected)
                .child("Connect")
                .on_click(cx.listener(|shell, _: &ClickEvent, _, cx| {
                    shell.connect(cx);
                    cx.stop_propagation();
                    cx.notify();
                })),
        )
}

fn field_row(
    cx: &mut Context<Shell>,
    label: &str,
    value: &str,
    which: Pane,
    current: Pane,
    row: f32,
    paint: Paint,
) -> gpui::Stateful<gpui::Div> {
    let on = current == which;
    div()
        .id(label.to_string())
        .h(px(row))
        .px(px(8.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_pointer()
        .when(on, |row| row.bg(paint.selected))
        .on_click(cx.listener(move |shell, _: &ClickEvent, _, cx| {
            shell.pane = which;
            cx.stop_propagation();
            cx.notify();
        }))
        .child(
            div()
                .w(px(72.0))
                .text_color(paint.secondary)
                .child(label.to_string()),
        )
        .child(value.to_string())
}

fn confirm_box(cx: &mut Context<Shell>, paint: Paint) -> gpui::Stateful<gpui::Div> {
    div()
        .id("confirm-quit")
        .absolute()
        .top(px(80.0))
        .left(px(80.0))
        .w(px(420.0))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .p(px(12.0))
        .bg(paint.raised)
        .border_1()
        .border_color(paint.border)
        .occlude()
        .on_click(cx.listener(|_shell, _: &ClickEvent, _, cx| {
            cx.stop_propagation();
        }))
        .child("Quit while a transfer is active?")
        .child("Enter confirms. Esc cancels. A bare q does not quit.")
}

fn settings_panel(
    cx: &mut Context<Shell>,
    settings: &SettingsView,
    row: f32,
    paint: Paint,
    scroll: &ScrollHandle,
) -> gpui::Stateful<gpui::Div> {
    let mut nav = div()
        .w(px(200.0))
        .h_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .border_r_1()
        .border_color(paint.border)
        .child(
            div()
                .id("settings-search")
                .h(px(36.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_color(paint.secondary)
                .on_click(cx.listener(|shell, _: &ClickEvent, _, cx| {
                    shell.query_on = true;
                    cx.stop_propagation();
                    cx.notify();
                }))
                .child(if settings.query.is_empty() {
                    "Search".to_string()
                } else {
                    settings.query.clone()
                }),
        );
    for (index, label) in PAGE_NAMES.iter().enumerate() {
        let on = settings.nav_on == Some(index);
        let mark = if on { ">" } else { " " };
        nav = nav.child(
            div()
                .id(("settings-nav", index))
                .h(px(32.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .cursor_pointer()
                .when(on, |row| row.bg(paint.selected))
                .on_click(cx.listener(move |shell, _: &ClickEvent, _, cx| {
                    shell.page = Page::from_index(index);
                    shell.query.clear();
                    shell.query_on = false;
                    cx.stop_propagation();
                    cx.notify();
                }))
                .child(format!("{mark} {label}")),
        );
    }

    let mut scroll_el = div()
        .id("settings-scroll")
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .track_scroll(scroll);
    if settings.page_name == "Themes" && !settings.editor {
        for (index, label, on) in &settings.theme_rows {
            let index = *index;
            scroll_el = scroll_el.child(
                div()
                    .id(("theme-row", index))
                    .h(px(28.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .when(*on, |row| row.bg(paint.selected))
                    .on_click(cx.listener(move |shell, _: &ClickEvent, _, cx| {
                        shell.select_theme(index);
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .child(label.clone()),
            );
        }
        scroll_el = scroll_el
            .child(settings_line(format!("Source: {}", settings.source), paint.secondary))
            .child(settings_line(settings.save_to.clone(), paint.text))
            .child(settings_line("Edit colors…".to_string(), paint.text))
            .child(settings_line("Save".to_string(), paint.text));
    } else if settings.page_name == "Themes" {
        scroll_el = scroll_el.child(editor_column(settings, paint));
    } else if settings.page_name == "Window" {
        scroll_el = scroll_el.child(window_column(cx, settings, paint));
    } else if settings.page_name == "Session" {
        scroll_el = scroll_el.child(session_column(settings, row, paint));
    } else if settings.page_name == "Help" {
        scroll_el = scroll_el.child(help_column(settings, paint));
    } else {
        scroll_el = scroll_el.child(settings_line("Results".to_string(), paint.text));
    }

    div()
        .id("settings-panel")
        .occlude()
        .absolute()
        .top(px(CHROME_PX))
        .right(px(0.0))
        .bottom(px(CHROME_PX))
        .w(gpui::relative(0.7))
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(paint.panel)
        .text_color(paint.text)
        .border_l_1()
        .border_color(paint.border)
        .on_click(cx.listener(|_shell, _: &ClickEvent, _, cx| {
            cx.stop_propagation();
        }))
        .child(
            div()
                .h(px(48.0))
                .px(px(16.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .border_b_1()
                .border_color(paint.border)
                .child("Settings")
                .child(div().flex_1())
                .child(div().text_color(paint.secondary).child("F1"))
                .child(
                    div()
                        .id("settings-close")
                        .px(px(8.0))
                        .cursor_pointer()
                        .child("Close")
                        .on_click(cx.listener(|shell, _: &ClickEvent, _, cx| {
                            shell.close_settings(true);
                            cx.stop_propagation();
                            cx.notify();
                        })),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .flex_row()
                .overflow_hidden()
                .child(nav)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .h_full()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .child(
                            div()
                                .h(px(28.0))
                                .px(px(8.0))
                                .flex()
                                .items_center()
                                .font_weight(FontWeight::BOLD)
                                .bg(paint.raised)
                                .border_b_1()
                                .border_color(paint.border)
                                .child(settings.page_name.clone()),
                        )
                        .child(
                            div()
                                .px(px(8.0))
                                .py(px(4.0))
                                .text_color(paint.secondary)
                                .child(settings.hint.clone()),
                        )
                        .child(scroll_el),
                ),
        )
}

fn editor_column(settings: &SettingsView, paint: Paint) -> gpui::Div {
    let mut col = div().flex().flex_col().p(px(8.0)).gap(px(4.0));
    col = col.child("Theme color editor".to_string());
    col = col.child(if settings.direct_editor {
        "Revert".to_string()
    } else {
        "Back".to_string()
    });
    for (_index, label, on) in &settings.editor_rows {
        col = col.child(
            div()
                .h(px(28.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .when(*on, |row| row.bg(paint.selected))
                .child(label.clone()),
        );
    }
    col.child(format!("Hex: {}", settings.hex))
        .child(settings.save_to.clone())
        .child("Save".to_string())
}

fn window_column(cx: &mut Context<Shell>, settings: &SettingsView, paint: Paint) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .p(px(8.0))
        .child(option_box(
            cx,
            "Density",
            &settings.density,
            settings.window_row == 0,
            0,
            paint,
        ))
        .child(option_box(
            cx,
            "Vim keys",
            &settings.vim,
            settings.window_row == 1,
            1,
            paint,
        ))
}

fn option_box(
    cx: &mut Context<Shell>,
    title: &str,
    value: &str,
    on: bool,
    row: usize,
    paint: Paint,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(("window-option", row))
        .border_1()
        .border_color(paint.border)
        .cursor_pointer()
        .when(on, |row| row.bg(paint.selected))
        .on_click(cx.listener(move |shell, _: &ClickEvent, _, cx| {
            shell.window_row = row;
            shell.toggle_window_row();
            cx.stop_propagation();
            cx.notify();
        }))
        .child(
            div()
                .h(px(28.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .font_weight(FontWeight::BOLD)
                .bg(paint.raised)
                .border_b_1()
                .border_color(paint.border)
                .child(title.to_string()),
        )
        .child(div().p(px(8.0)).child(value.to_string()))
}

fn session_column(settings: &SettingsView, row: f32, paint: Paint) -> gpui::Div {
    let mut col = div().flex().flex_col().p(px(8.0));
    for line in &settings.session {
        col = col.child(
            div()
                .h(px(row))
                .flex()
                .items_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(line.clone()),
        );
    }
    col.child(
        div()
            .mt(px(8.0))
            .text_color(paint.secondary)
            .child("Saved to ~/.config/ldnddev/dd_ftp.toml (not the theme file)"),
    )
    .child(
        div()
            .text_color(paint.secondary)
            .child("Tab cycle | Left/Right/Space change | Enter save | Esc cancel"),
    )
}

fn help_column(settings: &SettingsView, paint: Paint) -> gpui::Div {
    let mut body = div().flex().flex_col();
    let mut section = String::new();
    for line in &settings.help {
        if !line.contains('·') {
            if !section.is_empty() {
                body = body.child(div().h(px(12.0)));
            }
            section = line.clone();
            body = body.child(
                div()
                    .h(px(28.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .font_weight(FontWeight::BOLD)
                    .bg(paint.raised)
                    .border_b_1()
                    .border_color(paint.border)
                    .child(line.clone()),
            );
        } else {
            body = body.child(
                div()
                    .h(px(24.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .child(line.clone()),
            );
        }
    }
    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .p(px(8.0))
        .child(
            div()
                .border_1()
                .border_color(paint.border)
                .child(body),
        )
}

fn settings_line(text: String, color: gpui::Hsla) -> gpui::Div {
    div()
        .h(px(28.0))
        .px(px(8.0))
        .flex()
        .items_center()
        .text_color(color)
        .child(text)
}

fn theme_row_label(row: &PickerRow) -> String {
    match row {
        PickerRow::Follow { name } => format!("Follow desktop {name}"),
        PickerRow::Theme(theme) => theme.name.clone(),
        PickerRow::Custom => "Custom".to_string(),
    }
}

fn protocol_label(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Sftp => "sftp",
        Protocol::Ftp => "ftp",
        Protocol::Ftps => "ftps",
    }
}

fn default_port(protocol: Protocol) -> u16 {
    match protocol {
        Protocol::Sftp => 22,
        Protocol::Ftp => 21,
        Protocol::Ftps => 990,
    }
}

fn is_settings_toggle(key: &str) -> bool {
    matches!(key, "f1" | "f2" | "ctrl-/" | "ctrl-,")
}

fn editor_key(key: &str) -> Option<EditorKey> {
    match key {
        "up" | "k" => Some(EditorKey::Up),
        "down" | "j" => Some(EditorKey::Down),
        "left" | "h" => Some(EditorKey::Left),
        "right" | "l" => Some(EditorKey::Right),
        "tab" => Some(EditorKey::Tab),
        "enter" => Some(EditorKey::Enter),
        "escape" => Some(EditorKey::Esc),
        "backspace" => Some(EditorKey::Backspace),
        "g" => Some(EditorKey::Char('g')),
        "shift-g" => Some(EditorKey::Char('G')),
        "space" => Some(EditorKey::Char(' ')),
        other => {
            let mut chars = other.chars();
            let ch = chars.next()?;
            if chars.next().is_none() {
                Some(EditorKey::Char(ch))
            } else {
                None
            }
        }
    }
}

fn typed_char(key: &str) -> Option<char> {
    if key == "space" {
        return Some(' ');
    }
    let mut chars = key.chars();
    let ch = chars.next()?;
    if chars.next().is_some() || ch.is_control() {
        return None;
    }
    Some(ch)
}

fn is_paste(event: &KeyDownEvent) -> bool {
    let mods = event.keystroke.modifiers;
    (mods.control || mods.platform) && event.keystroke.key.eq_ignore_ascii_case("v")
}

fn chord(event: &KeyDownEvent) -> String {
    let mut parts = Vec::new();
    let mods = event.keystroke.modifiers;
    if mods.control || mods.platform {
        parts.push("ctrl".to_string());
    }
    if mods.shift {
        parts.push("shift".to_string());
    }
    if mods.alt {
        parts.push("alt".to_string());
    }
    parts.push(event.keystroke.key.to_ascii_lowercase());
    parts.join("-")
}
