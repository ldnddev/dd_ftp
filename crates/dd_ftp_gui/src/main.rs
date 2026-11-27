//! dd_ftp window. The terminal app stays `dd_ftp`; this binary is `dd_ftp-gui`.

mod bookmarks;
mod keys;
mod listing;
mod prefs;
mod session;
mod shell;
mod shortcuts;
mod theme_load;
mod vault;
mod windows;
mod xfer;

#[cfg(test)]
mod tests;

use gpui::{App, AppContext, Bounds, Focusable, px, size};
use gpui_component::Root;
use shell::{Shell, ShellOptions};

fn main() {
    if std::env::args()
        .skip(1)
        .any(|arg| arg == "--help" || arg == "-h")
    {
        print_help();
        return;
    }
    gpui_platform::application().run(|cx: &mut App| {
        gpui_component::init(cx);
        cx.spawn(async move |cx| {
            cx.update(|cx| {
                let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
                cx.open_window(windows::main_window_options(bounds), |window, cx| {
                    let view = cx.new(|cx| {
                        let shell = Shell::open(ShellOptions::production(), cx);
                        window.focus(&shell.focus_handle(cx), cx);
                        shell
                    });
                    cx.new(|cx| Root::new(view, window, cx))
                })
                .expect("open window");
            });
        })
        .detach();
    });
}

fn print_help() {
    println!(
        "\
dd_ftp

Usage: dd_ftp-gui [--help]

Open the dd_ftp window (1280×800).
  F1          Settings (Themes, Window, Session, Help)
  F3          Settings, Session page
  Ctrl+Q      Quit (confirms while a transfer is active)
  1 / 2 / 3   Local pane / remote pane / queue
  c           Connect, or disconnect when a session is open
  u / d       Upload the local selection / download the remote selection
  M           Move the focused pane's selection
  B           Save the form as a bookmark
  Ctrl+K      Check the keyring
  Ctrl+P      Choose a private key while the key field is focused
  Ctrl+C      Cancel in-flight transfers
  Up/Down     Move the focused listing
  Enter       Open a directory, or queue a file

Wayland or X11, and Vulkan (libvulkan1 and a driver)."
    );
}
