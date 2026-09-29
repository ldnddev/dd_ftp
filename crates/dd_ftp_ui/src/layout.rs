use std::path::{Component, Path, PathBuf};

use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
    Prompt,
    QcName,
    QcHost,
    QcPort,
    QcUsername,
    QcPassword,
    QcPrivateKey,
    QcPath,
    SettingsEditor,
    SettingsLastLocal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlId {
    QcProtocol,
    QcBrowseKey,
    BookmarkRow(usize),
    SettingsWorkers,
    SettingsHideDotfiles,
    SettingsOverwrite,
    SettingsAutoConnect,
    LocalPathSeg(usize),
    RemotePathSeg(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollRegion {
    ListLocal,
    ListRemote,
    Queue,
    Help,
    KeyPicker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    List(Pane),
    Scrollbar(ScrollRegion),
    Field(FieldId),
    Control(ControlId),
    KeyPickerList,
    QueueList,
}

#[derive(Debug, Clone, Copy)]
pub struct FieldRegion {
    pub id: FieldId,
    pub area: Rect,
    pub text_x: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct ControlRegion {
    pub id: ControlId,
    pub area: Rect,
}

#[derive(Debug, Default, Clone)]
pub struct LayoutMap {
    pub local_list: Rect,
    pub remote_list: Rect,
    pub local_scrollbar: Rect,
    pub remote_scrollbar: Rect,
    pub queue_scrollbar: Rect,
    pub help_scrollbar: Option<Rect>,
    pub queue: Rect,
    pub queue_list: Rect,
    pub queue_list_offset: usize,
    pub help: Option<Rect>,
    pub fields: Vec<FieldRegion>,
    pub controls: Vec<ControlRegion>,
    pub local_list_offset: usize,
    pub remote_list_offset: usize,
    pub key_picker_list: Rect,
    pub key_picker_scrollbar: Rect,
    pub key_picker_list_offset: usize,
    pub help_line_count: usize,
    pub queue_line_count: usize,
    pub local_path_targets: Vec<String>,
    pub remote_path_targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathSeg {
    pub label: String,
    pub target: String,
}

pub fn split_fs_path(path: &str) -> Vec<PathSeg> {
    let p = Path::new(path);
    let mut acc = PathBuf::new();
    let mut segs = Vec::new();
    for c in p.components() {
        acc.push(c);
        let label = match c {
            Component::RootDir => "/".to_string(),
            Component::Prefix(pr) => pr.as_os_str().to_string_lossy().into_owned(),
            Component::CurDir => ".".to_string(),
            Component::ParentDir => "..".to_string(),
            Component::Normal(s) => s.to_string_lossy().into_owned(),
        };
        segs.push(PathSeg {
            label,
            target: acc.to_string_lossy().into_owned(),
        });
    }
    if segs.is_empty() {
        segs.push(PathSeg {
            label: path.to_string(),
            target: path.to_string(),
        });
    }
    segs
}

pub fn split_remote_path(path: &str) -> Vec<PathSeg> {
    if path.is_empty() || path == "/" {
        return vec![PathSeg {
            label: "/".into(),
            target: "/".into(),
        }];
    }
    let mut segs = Vec::new();
    let mut acc = String::new();
    if path.starts_with('/') {
        acc = "/".into();
        segs.push(PathSeg {
            label: "/".into(),
            target: "/".into(),
        });
    }
    for part in path.split('/') {
        if part.is_empty() {
            continue;
        }
        acc = if acc == "/" {
            format!("/{part}")
        } else if acc.is_empty() {
            part.to_string()
        } else {
            format!("{acc}/{part}")
        };
        segs.push(PathSeg {
            label: part.to_string(),
            target: acc.clone(),
        });
    }
    segs
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathSegAction<'a> {
    Parent,
    Goto(&'a str),
}

pub fn path_seg_action(
    targets: &[String],
    index: usize,
    is_double: bool,
) -> Option<PathSegAction<'_>> {
    let target = targets.get(index)?;
    if is_double && index + 1 == targets.len() {
        Some(PathSegAction::Parent)
    } else {
        Some(PathSegAction::Goto(target.as_str()))
    }
}

fn contains(r: Rect, x: u16, y: u16) -> bool {
    r.width > 0 && r.height > 0 && x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

impl LayoutMap {
    /// Which scrollbar thumb/track the pointer is over, if any. Used to skip
    /// redraws when the mouse moves without changing hover chrome.
    pub fn hovered_scrollbar(&self, pos: Option<(u16, u16)>) -> Option<ScrollRegion> {
        let (x, y) = pos?;
        if contains(self.key_picker_scrollbar, x, y) {
            Some(ScrollRegion::KeyPicker)
        } else if contains(self.local_scrollbar, x, y) {
            Some(ScrollRegion::ListLocal)
        } else if contains(self.remote_scrollbar, x, y) {
            Some(ScrollRegion::ListRemote)
        } else if contains(self.queue_scrollbar, x, y) {
            Some(ScrollRegion::Queue)
        } else if self.help_scrollbar.is_some_and(|h| contains(h, x, y)) {
            Some(ScrollRegion::Help)
        } else {
            None
        }
    }
}

/// Hit-test a point. Modal regions (fields, controls, help) win over the
/// background lists so a wheel/click over an open modal never leaks through.
pub fn hit_test(m: &LayoutMap, x: u16, y: u16) -> Option<Region> {
    if contains(m.key_picker_scrollbar, x, y) {
        return Some(Region::Scrollbar(ScrollRegion::KeyPicker));
    }
    if contains(m.key_picker_list, x, y) {
        return Some(Region::KeyPickerList);
    }
    for f in &m.fields {
        if contains(f.area, x, y) {
            return Some(Region::Field(f.id));
        }
    }
    for c in &m.controls {
        if contains(c.area, x, y) {
            return Some(Region::Control(c.id));
        }
    }
    if let Some(h) = m.help_scrollbar {
        if contains(h, x, y) {
            return Some(Region::Scrollbar(ScrollRegion::Help));
        }
    }
    if let Some(h) = m.help {
        if contains(h, x, y) {
            return Some(Region::Scrollbar(ScrollRegion::Help));
        }
    }
    if contains(m.local_scrollbar, x, y) {
        return Some(Region::Scrollbar(ScrollRegion::ListLocal));
    }
    if contains(m.remote_scrollbar, x, y) {
        return Some(Region::Scrollbar(ScrollRegion::ListRemote));
    }
    if contains(m.queue_scrollbar, x, y) {
        return Some(Region::Scrollbar(ScrollRegion::Queue));
    }
    if contains(m.queue_list, x, y) || contains(m.queue, x, y) {
        return Some(Region::QueueList);
    }
    if contains(m.local_list, x, y) {
        return Some(Region::List(Pane::Local));
    }
    if contains(m.remote_list, x, y) {
        return Some(Region::List(Pane::Remote));
    }
    None
}

/// Map an x column to a char index within a field's text, clamped to [0, len].
pub fn char_index_at(f: &FieldRegion, x: u16, len: usize) -> usize {
    if x <= f.text_x {
        return 0;
    }
    ((x - f.text_x) as usize).min(len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn r(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect::new(x, y, w, h)
    }

    #[test]
    fn hit_test_prefers_modal_field_over_list() {
        let mut m = LayoutMap {
            local_list: r(0, 3, 40, 20),
            ..Default::default()
        };
        m.fields.push(FieldRegion {
            id: FieldId::Prompt,
            area: r(10, 10, 20, 1),
            text_x: 18,
        });
        // point inside both list and the field -> field wins (modal precedence)
        assert_eq!(hit_test(&m, 12, 10), Some(Region::Field(FieldId::Prompt)));
        // point only in list
        assert_eq!(hit_test(&m, 2, 5), Some(Region::List(Pane::Local)));
        // point in nothing
        assert_eq!(hit_test(&m, 200, 200), None);
    }

    #[test]
    fn hit_test_prefers_key_picker_over_qc_field() {
        let mut m = LayoutMap {
            key_picker_list: r(8, 8, 30, 12),
            ..Default::default()
        };
        m.fields.push(FieldRegion {
            id: FieldId::QcPrivateKey,
            area: r(10, 22, 20, 3),
            text_x: 11,
        });
        assert_eq!(hit_test(&m, 12, 10), Some(Region::KeyPickerList));
        assert_eq!(
            hit_test(&m, 12, 23),
            Some(Region::Field(FieldId::QcPrivateKey))
        );
    }

    #[test]
    fn hit_test_queue_list_prefers_scrollbar() {
        let m = LayoutMap {
            queue: r(0, 20, 40, 8),
            queue_list: r(1, 22, 38, 5),
            queue_scrollbar: r(39, 20, 1, 8),
            ..Default::default()
        };
        assert_eq!(
            hit_test(&m, 39, 22),
            Some(Region::Scrollbar(ScrollRegion::Queue))
        );
        assert_eq!(hit_test(&m, 5, 23), Some(Region::QueueList));
        assert_eq!(hit_test(&m, 2, 21), Some(Region::QueueList));
    }

    #[test]
    fn hovered_scrollbar_ignores_list_body() {
        let m = LayoutMap {
            local_list: r(0, 3, 40, 20),
            local_scrollbar: r(39, 3, 1, 20),
            remote_scrollbar: r(79, 3, 1, 20),
            ..Default::default()
        };
        assert_eq!(m.hovered_scrollbar(None), None);
        assert_eq!(m.hovered_scrollbar(Some((10, 10))), None);
        assert_eq!(
            m.hovered_scrollbar(Some((39, 10))),
            Some(ScrollRegion::ListLocal)
        );
        assert_eq!(
            m.hovered_scrollbar(Some((79, 5))),
            Some(ScrollRegion::ListRemote)
        );
        assert_ne!(
            m.hovered_scrollbar(Some((39, 10))),
            m.hovered_scrollbar(Some((10, 10)))
        );
        assert_eq!(
            m.hovered_scrollbar(Some((39, 10))),
            m.hovered_scrollbar(Some((39, 12)))
        );
    }

    #[test]
    fn char_index_clamps_to_bounds() {
        let f = FieldRegion {
            id: FieldId::Prompt,
            area: r(10, 10, 20, 1),
            text_x: 12,
        };
        assert_eq!(char_index_at(&f, 12, 5), 0); // at text start, len 5
        assert_eq!(char_index_at(&f, 15, 5), 3); // 3 chars in
        assert_eq!(char_index_at(&f, 99, 5), 5); // past end clamps to len
        assert_eq!(char_index_at(&f, 0, 5), 0); // left of field clamps to 0
    }

    #[test]
    fn split_remote_path_nested_and_root() {
        let segs = split_remote_path("/var/www/html");
        assert_eq!(
            segs.iter().map(|s| s.target.as_str()).collect::<Vec<_>>(),
            vec!["/", "/var", "/var/www", "/var/www/html"]
        );
        assert_eq!(segs[1].label, "var");
        let root = split_remote_path("/");
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].target, "/");
    }

    #[test]
    fn split_fs_path_nested() {
        let segs = split_fs_path("/home/demo/www");
        assert_eq!(segs.first().map(|s| s.label.as_str()), Some("/"));
        assert_eq!(
            segs.last().map(|s| s.target.as_str()),
            Some("/home/demo/www")
        );
        assert_eq!(segs.last().map(|s| s.label.as_str()), Some("www"));
    }

    #[test]
    fn path_seg_action_double_last_is_parent() {
        let targets = vec!["/".into(), "/home".into(), "/home/demo".into()];
        assert_eq!(
            path_seg_action(&targets, 1, false),
            Some(PathSegAction::Goto("/home"))
        );
        assert_eq!(
            path_seg_action(&targets, 2, true),
            Some(PathSegAction::Parent)
        );
        assert_eq!(
            path_seg_action(&targets, 2, false),
            Some(PathSegAction::Goto("/home/demo"))
        );
        assert!(path_seg_action(&targets, 9, false).is_none());
    }
}
