use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane { Local, Remote }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
    Prompt,
    QcName, QcHost, QcPort, QcUsername, QcPassword, QcPrivateKey, QcPath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlId {
    QcProtocol,
    BookmarkRow(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollRegion { ListLocal, ListRemote, Queue, Help }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    List(Pane),
    Scrollbar(ScrollRegion),
    Field(FieldId),
    Control(ControlId),
}

#[derive(Debug, Clone, Copy)]
pub struct FieldRegion { pub id: FieldId, pub area: Rect, pub text_x: u16 }

#[derive(Debug, Clone, Copy)]
pub struct ControlRegion { pub id: ControlId, pub area: Rect }

#[derive(Debug, Default, Clone)]
pub struct LayoutMap {
    pub local_list: Rect,
    pub remote_list: Rect,
    pub local_scrollbar: Rect,
    pub remote_scrollbar: Rect,
    pub queue_scrollbar: Rect,
    pub help_scrollbar: Option<Rect>,
    pub queue: Rect,
    pub help: Option<Rect>,
    pub fields: Vec<FieldRegion>,
    pub controls: Vec<ControlRegion>,
}

fn contains(r: Rect, x: u16, y: u16) -> bool {
    r.width > 0 && r.height > 0
        && x >= r.x && x < r.x + r.width
        && y >= r.y && y < r.y + r.height
}

/// Hit-test a point. Modal regions (fields, controls, help) win over the
/// background lists so a wheel/click over an open modal never leaks through.
pub fn hit_test(m: &LayoutMap, x: u16, y: u16) -> Option<Region> {
    for f in &m.fields {
        if contains(f.area, x, y) { return Some(Region::Field(f.id)); }
    }
    for c in &m.controls {
        if contains(c.area, x, y) { return Some(Region::Control(c.id)); }
    }
    if let Some(h) = m.help_scrollbar {
        if contains(h, x, y) { return Some(Region::Scrollbar(ScrollRegion::Help)); }
    }
    if let Some(h) = m.help {
        if contains(h, x, y) { return Some(Region::Scrollbar(ScrollRegion::Help)); }
    }
    if contains(m.local_scrollbar, x, y) { return Some(Region::Scrollbar(ScrollRegion::ListLocal)); }
    if contains(m.remote_scrollbar, x, y) { return Some(Region::Scrollbar(ScrollRegion::ListRemote)); }
    if contains(m.queue_scrollbar, x, y) { return Some(Region::Scrollbar(ScrollRegion::Queue)); }
    if contains(m.queue, x, y) { return Some(Region::Scrollbar(ScrollRegion::Queue)); }
    if contains(m.local_list, x, y) { return Some(Region::List(Pane::Local)); }
    if contains(m.remote_list, x, y) { return Some(Region::List(Pane::Remote)); }
    None
}

/// Map an x column to a char index within a field's text, clamped to [0, len].
pub fn char_index_at(f: &FieldRegion, x: u16, len: usize) -> usize {
    if x <= f.text_x { return 0; }
    ((x - f.text_x) as usize).min(len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn r(x: u16, y: u16, w: u16, h: u16) -> Rect { Rect::new(x, y, w, h) }

    #[test]
    fn hit_test_prefers_modal_field_over_list() {
        let mut m = LayoutMap { local_list: r(0, 3, 40, 20), ..Default::default() };
        m.fields.push(FieldRegion { id: FieldId::Prompt, area: r(10, 10, 20, 1), text_x: 18 });
        // point inside both list and the field -> field wins (modal precedence)
        assert_eq!(hit_test(&m, 12, 10), Some(Region::Field(FieldId::Prompt)));
        // point only in list
        assert_eq!(hit_test(&m, 2, 5), Some(Region::List(Pane::Local)));
        // point in nothing
        assert_eq!(hit_test(&m, 200, 200), None);
    }

    #[test]
    fn char_index_clamps_to_bounds() {
        let f = FieldRegion { id: FieldId::Prompt, area: r(10, 10, 20, 1), text_x: 12 };
        assert_eq!(char_index_at(&f, 12, 5), 0);   // at text start, len 5
        assert_eq!(char_index_at(&f, 15, 5), 3);   // 3 chars in
        assert_eq!(char_index_at(&f, 99, 5), 5);   // past end clamps to len
        assert_eq!(char_index_at(&f, 0, 5), 0);    // left of field clamps to 0
    }
}
