//! App id and window options.
//!
//! On Wayland `app_id` is the xdg app id. On X11 it is the window class.

use gpui::{Bounds, Pixels, SharedString, TitlebarOptions, WindowBounds, WindowOptions};

pub const MAIN_APP_ID: &str = "dev.ldnddev.dd_ftp";

pub fn main_window_options(bounds: Bounds<Pixels>) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(SharedString::from("dd_ftp")),
            ..Default::default()
        }),
        focus: true,
        show: true,
        app_id: Some(MAIN_APP_ID.to_string()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, px, size};

    #[gpui::test]
    fn window_is_1280_by_800_with_the_app_id(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let options =
                main_window_options(Bounds::centered(None, size(px(1280.0), px(800.0)), cx));
            assert_eq!(options.app_id.as_deref(), Some(MAIN_APP_ID));
            let bounds = options.window_bounds.expect("bounds").get_bounds();
            assert_eq!(bounds.size, size(px(1280.0), px(800.0)));
            let title = options
                .titlebar
                .as_ref()
                .and_then(|bar| bar.title.as_ref())
                .map(SharedString::to_string);
            assert_eq!(title.as_deref(), Some("dd_ftp"));
        });
    }
}
