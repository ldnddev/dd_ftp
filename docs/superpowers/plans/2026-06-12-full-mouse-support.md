# Full Mouse Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add complete mouse interaction to the dd_ftp TUI — wheel scroll, click-to-focus/select, double-click-to-enter, scrollbar drag, and in-field text selection with a real cursor model.

**Architecture:** `render` populates a `LayoutMap` (hit-test geometry) via an out-param each frame; the CLI caches it and translates `Event::Mouse` into existing/new `Action`s. `reduce` stays pure (no Rect, no timing). A reusable `TextField` (cursor + selection) backs all editable inputs. Drag and double-click state live in the CLI event loop using `Instant`.

**Tech Stack:** Rust, ratatui, crossterm, tokio. Workspace crates: `dd_ftp_app` (state/actions/reducer, pure), `dd_ftp_ui` (render + new layout/hit-test module), `dd_ftp_cli` (event loop).

**Spec:** `docs/superpowers/specs/2026-06-12-full-mouse-support-design.md`

---

## File Structure

- `crates/dd_ftp_ui/src/layout.rs` — **new**. `LayoutMap`, `Region`, `FieldId`, `ControlId`, `FieldRegion`, `ControlRegion`, and pure `hit_test` + `char_index_at` helpers. Unit-tested.
- `crates/dd_ftp_ui/src/render.rs` — modify: `render` gains `map: &mut LayoutMap` out-param; populate regions where each widget is drawn.
- `crates/dd_ftp_ui/src/lib.rs` — modify: `pub mod layout;` + re-exports.
- `crates/dd_ftp_app/src/text_field.rs` — **new**. `TextField` struct + pure edit/selection methods. Unit-tested.
- `crates/dd_ftp_app/src/state.rs` — modify: `prompt_value: String` → `prompt_value: TextField`; add `qc_field: TextField` (active quick-connect field editor) + helpers.
- `crates/dd_ftp_app/src/actions.rs` — modify: add field cursor/selection actions.
- `crates/dd_ftp_app/src/reducer.rs` — modify: handle new actions; route input/backspace through `TextField`.
- `crates/dd_ftp_app/src/lib.rs` — modify: `mod text_field; pub use text_field::TextField;`
- `crates/dd_ftp_cli/src/main.rs` — modify: cache `LayoutMap`; replace the `Moved`-only mouse arm with full translation; add `MouseState` (drag + double-click).

Staging order (each stage = reviewable, compiling, testable): **1** infra → **2** scroll → **3** click/select/controls → **4** scrollbar drag → **5** TextField + field selection + keyboard editing.

---

## STAGE 1 — LayoutMap infrastructure (no behavior change)

### Task 1: LayoutMap types + hit_test (pure, dd_ftp_ui)

**Files:**
- Create: `crates/dd_ftp_ui/src/layout.rs`
- Modify: `crates/dd_ftp_ui/src/lib.rs`
- Test: in `crates/dd_ftp_ui/src/layout.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Append to `crates/dd_ftp_ui/src/layout.rs` (create the file with this test first; types come next step):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn r(x: u16, y: u16, w: u16, h: u16) -> Rect { Rect::new(x, y, w, h) }

    #[test]
    fn hit_test_prefers_modal_field_over_list() {
        let mut m = LayoutMap::default();
        m.local_list = r(0, 3, 40, 20);
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dd_ftp_ui layout`
Expected: FAIL — `LayoutMap`, `Region`, etc. not found / file not a module.

- [ ] **Step 3: Write the types + helpers**

Prepend to `crates/dd_ftp_ui/src/layout.rs` (above the test module):

```rust
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
```

- [ ] **Step 4: Register module + run tests**

In `crates/dd_ftp_ui/src/lib.rs` add `pub mod layout;` and `pub use layout::{LayoutMap, Region, Pane, ScrollRegion, FieldId, ControlId, FieldRegion, ControlRegion, hit_test, char_index_at};`

Run: `cargo test -p dd_ftp_ui layout`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/dd_ftp_ui/src/layout.rs crates/dd_ftp_ui/src/lib.rs
git commit -m "feat(ui): LayoutMap + hit_test geometry helpers"
```

### Task 2: render populates LayoutMap via out-param

**Files:**
- Modify: `crates/dd_ftp_ui/src/render.rs`
- Modify: `crates/dd_ftp_cli/src/main.rs:83` (call site)

- [ ] **Step 1: Change render signature**

In `render.rs`, change `pub fn render(frame: &mut Frame, app: &AppState) {` to:

```rust
pub fn render(frame: &mut Frame, app: &AppState, map: &mut LayoutMap) {
    *map = LayoutMap::default();
```

Add `use crate::layout::{LayoutMap, FieldId, FieldRegion, ControlId, ControlRegion};` to the imports.

- [ ] **Step 2: Record the list + queue rects**

Just after `panes` is split (`render.rs:97-100`), add:

```rust
    map.local_list = panes[0];
    map.remote_list = panes[1];
```

Just after `queue_area` is bound (`render.rs:75`), add:

```rust
    map.queue = queue_area;
```

- [ ] **Step 3: Record scrollbar rects**

The list scrollbars are drawn around `render.rs:256-272`. Each `render_scrollbar` call draws into a derived area = rightmost column of the pane. Record the same 1-column rect. After the local list scrollbar call, add:

```rust
    map.local_scrollbar = Rect { x: panes[0].x + panes[0].width.saturating_sub(1), y: panes[0].y, width: 1, height: panes[0].height };
    map.remote_scrollbar = Rect { x: panes[1].x + panes[1].width.saturating_sub(1), y: panes[1].y, width: 1, height: panes[1].height };
    map.queue_scrollbar = Rect { x: queue_area.x + queue_area.width.saturating_sub(1), y: queue_area.y, width: 1, height: queue_area.height };
```

(`Rect` is already imported in `render.rs:4`.)

- [ ] **Step 4: Update the call site**

In `main.rs`, replace the draw line (`main.rs:83`):

```rust
        terminal.draw(|f| dd_ftp_ui::render(f, app))?;
```

with:

```rust
        let mut layout_map = dd_ftp_ui::LayoutMap::default();
        terminal.draw(|f| dd_ftp_ui::render(f, app, &mut layout_map))?;
        app_layout = layout_map;
```

And declare `let mut app_layout = dd_ftp_ui::LayoutMap::default();` just before the `loop {` at `main.rs:81`.

- [ ] **Step 5: Build + verify no behavior change**

Run: `cargo build -p dd_ftp_cli && cargo clippy -p dd_ftp_ui --all-targets`
Expected: compiles clean. Run `cargo run -p dd_ftp_cli`, confirm UI looks identical, quit with `q`.

- [ ] **Step 6: Commit**

```bash
git add crates/dd_ftp_ui/src/render.rs crates/dd_ftp_cli/src/main.rs
git commit -m "feat(ui): render emits LayoutMap; CLI caches it"
```

---

## STAGE 2 — Wheel scroll (surfaces A-scroll + B)

### Task 3: Wheel scroll over lists, queue, help

**Files:**
- Modify: `crates/dd_ftp_cli/src/main.rs:599-603` (mouse arm)

Reuse existing paths (spec §4): list scroll = `Action::SelectUp/SelectDown` ×3 against the pane under the cursor; queue/help = mutate `queue_scroll`/`help_scroll` like the existing key handlers (`main.rs:249-252`, `460-467`).

- [ ] **Step 1: Replace the mouse arm**

Replace `main.rs:599-603` with:

```rust
            Event::Mouse(mouse) => {
                use dd_ftp_ui::{hit_test, Region, Pane, ScrollRegion};
                let (mx, my) = (mouse.column, mouse.row);
                match mouse.kind {
                    MouseEventKind::Moved => {
                        app.mouse_pos = Some((mx, my));
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        let up = matches!(mouse.kind, MouseEventKind::ScrollUp);
                        match hit_test(&app_layout, mx, my) {
                            Some(Region::List(pane)) => {
                                let prev = app.focus;
                                app.focus = match pane { Pane::Local => FocusPane::Local, Pane::Remote => FocusPane::Remote };
                                for _ in 0..3 {
                                    reduce(app, if up { Action::SelectUp } else { Action::SelectDown });
                                }
                                app.focus = if app.connected || pane == Pane::Local { app.focus } else { prev };
                            }
                            Some(Region::Scrollbar(ScrollRegion::Queue)) => {
                                if up { app.queue_scroll = app.queue_scroll.saturating_sub(3); }
                                else { app.queue_scroll = app.queue_scroll.saturating_add(3); }
                            }
                            Some(Region::Scrollbar(ScrollRegion::Help)) => {
                                if up { app.help_scroll = app.help_scroll.saturating_sub(3); }
                                else { app.help_scroll = app.help_scroll.saturating_add(3); }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
```

(Note: wheel-over-list re-points `focus` to the hovered pane so `SelectUp/Down` act on it; this matches click-to-focus added in Stage 3.)

- [ ] **Step 2: Build**

Run: `cargo build -p dd_ftp_cli`
Expected: compiles.

- [ ] **Step 3: Manual verify**

Run `cargo run -p dd_ftp_cli`. Scroll wheel over the local list → selection moves 3/notch and viewport follows. Open help (`F1`), scroll wheel → help scrolls. Tab to queue with items, scroll over queue → queue scrolls. Expected: all three respond; scrolling over empty regions does nothing.

- [ ] **Step 4: Commit**

```bash
git add crates/dd_ftp_cli/src/main.rs
git commit -m "feat(cli): mouse wheel scrolls list/queue/help"
```

---

## STAGE 3 — Click to focus / select / activate (surfaces A-click + E)

### Task 4: Record field + control regions in render

**Files:**
- Modify: `crates/dd_ftp_ui/src/render.rs` (quick-connect ~520-630, prompt ~830-840, bookmarks list, protocol toggle)

- [ ] **Step 1: Record the prompt field**

Where the prompt input line is rendered (`render.rs:835`, inside the prompt modal block), capture the input area `Rect` used for that paragraph (name it `prompt_area` if not already), then:

```rust
    map.fields.push(FieldRegion {
        id: FieldId::Prompt,
        area: prompt_area,
        text_x: prompt_area.x + 9, // after " Name: " label; match the label width actually rendered
    });
```

Adjust `text_x` to the real column where editable text begins for the prompt label in this codebase (inspect the rendered label span width at `render.rs:830-836`).

- [ ] **Step 2: Record quick-connect fields + protocol control**

In the quick-connect render loop (`render.rs:530-630`, which iterates `(QuickConnectField, label, value)` tuples), each field is drawn into a row `Rect` (call it `field_area`). For each non-protocol field, push a `FieldRegion` mapping the `QuickConnectField` to the matching `FieldId`:

```rust
    let fid = match *field {
        QuickConnectField::Name => Some(FieldId::QcName),
        QuickConnectField::Host => Some(FieldId::QcHost),
        QuickConnectField::Port => Some(FieldId::QcPort),
        QuickConnectField::Username => Some(FieldId::QcUsername),
        QuickConnectField::Password => Some(FieldId::QcPassword),
        QuickConnectField::PrivateKey => Some(FieldId::QcPrivateKey),
        QuickConnectField::Path => Some(FieldId::QcPath),
        QuickConnectField::Protocol => None,
    };
    if let Some(fid) = fid {
        map.fields.push(FieldRegion { id: fid, area: field_area, text_x: field_area.x + label_width });
    } else {
        map.controls.push(ControlRegion { id: ControlId::QcProtocol, area: field_area });
    }
```

`label_width` = the column offset where the value begins (the label span length already computed for rendering each row). Use the existing value-span start.

- [ ] **Step 3: Record bookmark rows**

In the bookmarks modal render, each list row maps to a `Rect`. For row index `i`, push `ControlRegion { id: ControlId::BookmarkRow(i), area: row_rect }`. If the bookmarks list uses a single `List` widget without per-row rects, compute each row as `Rect { x: list.x, y: list.y + i as u16, width: list.width, height: 1 }` for `i` in `0..visible_count`.

- [ ] **Step 4: Build**

Run: `cargo build -p dd_ftp_ui`
Expected: compiles. (No tests here — geometry is exercised via hit_test unit tests already and manual clicks next.)

- [ ] **Step 5: Commit**

```bash
git add crates/dd_ftp_ui/src/render.rs
git commit -m "feat(ui): record field + control regions in LayoutMap"
```

### Task 5: Click handling — focus, select, double-click enter, controls

**Files:**
- Modify: `crates/dd_ftp_cli/src/main.rs` (add `MouseState`, extend mouse arm)

Double-click needs timing → lives in the CLI via `Instant`. Enter-directory reuses the existing `navigate_into_directory(app, session)` (`main.rs:610`).

- [ ] **Step 1: Add MouseState + import Instant**

Add to the `use std::...time::` import (`main.rs:8`): `time::{Duration, Instant}`. Above the `loop {` (near `app_layout`), add:

```rust
        let mut last_click: Option<(u16, u16, Instant)> = None;
```

- [ ] **Step 2: Handle left button down in the mouse arm**

Inside the `Event::Mouse` arm's `match mouse.kind`, add a `MouseEventKind::Down(crossterm::event::MouseButton::Left) =>` branch:

```rust
                    MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                        let now = Instant::now();
                        let is_double = last_click
                            .map(|(lx, ly, t)| lx == mx && ly == my && now.duration_since(t) < Duration::from_millis(300))
                            .unwrap_or(false);
                        last_click = Some((mx, my, now));
                        match hit_test(&app_layout, mx, my) {
                            Some(Region::List(pane)) => {
                                app.focus = match pane { Pane::Local => FocusPane::Local, Pane::Remote => FocusPane::Remote };
                                // row index = my - list.y, clamped to entry count
                                let (list_rect, len) = match pane {
                                    Pane::Local => (app_layout.local_list, app.local_entries.len()),
                                    Pane::Remote => (app_layout.remote_list, app.remote_entries.len()),
                                };
                                if my >= list_rect.y {
                                    let idx = (my - list_rect.y) as usize;
                                    if idx < len {
                                        match pane {
                                            Pane::Local => app.selected_local = idx,
                                            Pane::Remote => app.selected_remote = idx,
                                        }
                                        if is_double {
                                            navigate_into_directory(app, session).await;
                                        }
                                    }
                                }
                            }
                            Some(Region::Control(ControlId::QcProtocol)) => {
                                reduce(app, Action::QuickConnectSetProtocolNext);
                            }
                            Some(Region::Control(ControlId::BookmarkRow(i))) => {
                                app.selected_bookmark = i.min(app.bookmarks.len().saturating_sub(1));
                                if is_double {
                                    // mirror the keyboard "connect to selected bookmark" path
                                    if let Some(bm) = app.bookmarks.get(app.selected_bookmark).cloned() {
                                        let hydrated = hydrate_password_from_keyring(app, bm, "bookmark-dblclick");
                                        reduce(app, Action::QuickConnectSetFromBookmark(hydrated));
                                        reduce(app, Action::ToggleBookmarks);
                                        reduce(app, Action::ToggleQuickConnect);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
```

Import `ControlId` in the `use dd_ftp_ui::{...}` line at the top of the mouse arm (add `ControlId`).

- [ ] **Step 3: Build**

Run: `cargo build -p dd_ftp_cli`
Expected: compiles. If `hydrate_password_from_keyring`/bookmark-connect signature differs, match the exact existing keyboard handler for "connect to selected bookmark" (search `QuickConnectSetFromBookmark` call sites in `main.rs`) and copy that sequence verbatim.

- [ ] **Step 4: Manual verify**

Run `cargo run -p dd_ftp_cli`. Single-click a file row → that pane gains focus, row highlights. Double-click a directory row → enters it. Open bookmarks (`m`), single-click a row → selects, double-click → loads into quick connect. Open quick connect, click the protocol field → cycles SFTP→FTP→FTPS.

- [ ] **Step 5: Commit**

```bash
git add crates/dd_ftp_cli/src/main.rs
git commit -m "feat(cli): mouse click focus/select, double-click enter, control clicks"
```

---

## STAGE 4 — Scrollbar drag (surface C)

### Task 6: Drag the scrollbar thumb to scroll

**Files:**
- Modify: `crates/dd_ftp_cli/src/main.rs` (drag origin state + Down/Drag/Up on scrollbars)

- [ ] **Step 1: Add drag state**

Next to `last_click`, add:

```rust
        let mut drag: Option<dd_ftp_ui::ScrollRegion> = None;
```

- [ ] **Step 2: Begin drag on scrollbar down**

In the existing `MouseEventKind::Down(Left)` branch (Task 5), add at the top of the `match hit_test` a `Some(Region::Scrollbar(sr))` arm BEFORE the list arm:

```rust
                            Some(Region::Scrollbar(sr)) => {
                                drag = Some(sr);
                                apply_scrollbar_drag(app, &app_layout, sr, my);
                            }
```

- [ ] **Step 3: Continue/end drag**

Add two branches to the outer `match mouse.kind`:

```rust
                    MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                        if let Some(sr) = drag {
                            apply_scrollbar_drag(app, &app_layout, sr, my);
                        }
                    }
                    MouseEventKind::Up(_) => {
                        drag = None;
                    }
```

- [ ] **Step 4: Implement apply_scrollbar_drag**

Add this free function near `navigate_into_directory` in `main.rs`:

```rust
fn apply_scrollbar_drag(
    app: &mut AppState,
    layout: &dd_ftp_ui::LayoutMap,
    sr: dd_ftp_ui::ScrollRegion,
    my: u16,
) {
    use dd_ftp_ui::ScrollRegion;
    // fraction of the track the cursor is at (0.0 top .. 1.0 bottom)
    let track = match sr {
        ScrollRegion::ListLocal => layout.local_scrollbar,
        ScrollRegion::ListRemote => layout.remote_scrollbar,
        ScrollRegion::Queue => layout.queue_scrollbar,
        ScrollRegion::Help => layout.help_scrollbar.unwrap_or_default(),
    };
    if track.height == 0 { return; }
    let rel = my.saturating_sub(track.y).min(track.height.saturating_sub(1));
    let frac = rel as f32 / track.height.saturating_sub(1).max(1) as f32;
    match sr {
        ScrollRegion::ListLocal => {
            let n = app.local_entries.len().saturating_sub(1);
            app.selected_local = (frac * n as f32).round() as usize;
        }
        ScrollRegion::ListRemote => {
            let n = app.remote_entries.len().saturating_sub(1);
            app.selected_remote = (frac * n as f32).round() as usize;
        }
        ScrollRegion::Queue => {
            // queue_scroll max is bounded by render's clamp; approximate via row count
            let n = app.queue.pending.len() + app.queue.active.len() + app.queue.completed.len() + app.queue.failed.len();
            app.queue_scroll = (frac * n as f32).round() as usize;
        }
        ScrollRegion::Help => {
            // help has no known total here; nudge proportionally to track rows
            app.help_scroll = (frac * track.height as f32).round() as usize;
        }
    }
}
```

(`Default::default()` for `Rect` is available; `unwrap_or_default()` yields a 0-size rect that the `height == 0` guard rejects.)

- [ ] **Step 5: Build + manual verify**

Run: `cargo build -p dd_ftp_cli`. Then `cargo run -p dd_ftp_cli` with a long file list: click-drag the right-edge scrollbar thumb up/down → selection/viewport tracks the drag. Release → drag ends; moving the mouse afterward does not scroll.

- [ ] **Step 6: Commit**

```bash
git add crates/dd_ftp_cli/src/main.rs
git commit -m "feat(cli): drag scrollbar thumb to scroll lists/queue/help"
```

---

## STAGE 5 — TextField model + field selection + keyboard editing (surface D)

### Task 7: TextField struct (pure, dd_ftp_app)

**Files:**
- Create: `crates/dd_ftp_app/src/text_field.rs`
- Modify: `crates/dd_ftp_app/src/lib.rs`
- Test: in `text_field.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write failing tests**

Create `crates/dd_ftp_app/src/text_field.rs`:

```rust
//! Reusable single-line text editor model: value + cursor + optional selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextField {
    pub value: String,
    pub cursor: usize,        // char index in 0..=len
    pub anchor: Option<usize>, // Some => active selection between anchor and cursor
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tf(s: &str, cursor: usize) -> TextField {
        TextField { value: s.to_string(), cursor, anchor: None }
    }

    #[test]
    fn insert_at_cursor() {
        let mut f = tf("ac", 1);
        f.insert_char('b');
        assert_eq!(f.value, "abc");
        assert_eq!(f.cursor, 2);
    }

    #[test]
    fn backspace_removes_left_of_cursor() {
        let mut f = tf("abc", 2);
        f.backspace();
        assert_eq!(f.value, "ac");
        assert_eq!(f.cursor, 1);
    }

    #[test]
    fn typing_replaces_active_selection() {
        let mut f = TextField { value: "hello".into(), cursor: 4, anchor: Some(1) };
        f.insert_char('X'); // selection [1,4) = "ell" replaced
        assert_eq!(f.value, "hXo");
        assert_eq!(f.cursor, 2);
        assert_eq!(f.anchor, None);
    }

    #[test]
    fn backspace_deletes_active_selection() {
        let mut f = TextField { value: "hello".into(), cursor: 1, anchor: Some(4) };
        f.backspace();
        assert_eq!(f.value, "ho");
        assert_eq!(f.cursor, 1);
        assert_eq!(f.anchor, None);
    }

    #[test]
    fn move_right_with_shift_extends_selection() {
        let mut f = tf("abc", 0);
        f.move_cursor(1, true);
        assert_eq!(f.anchor, Some(0));
        assert_eq!(f.cursor, 1);
        f.move_cursor(1, false); // no shift collapses
        assert_eq!(f.anchor, None);
    }

    #[test]
    fn set_and_drag_cursor_clamp() {
        let mut f = tf("abc", 0);
        f.set_cursor(99);
        assert_eq!(f.cursor, 3);
        f.begin_drag(1);
        assert_eq!(f.anchor, Some(1));
        f.extend_drag(99);
        assert_eq!(f.cursor, 3);
        assert_eq!(f.anchor, Some(1));
    }

    #[test]
    fn delete_word_left() {
        let mut f = tf("foo bar", 7);
        f.delete_word_left();
        assert_eq!(f.value, "foo ");
        assert_eq!(f.cursor, 4);
    }

    #[test]
    fn home_end() {
        let mut f = tf("abc", 1);
        f.move_home(false);
        assert_eq!(f.cursor, 0);
        f.move_end(false);
        assert_eq!(f.cursor, 3);
    }
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p dd_ftp_app text_field`
Expected: FAIL — methods not defined.

- [ ] **Step 3: Implement the methods**

Insert this `impl` block in `text_field.rs` between the struct and the test module:

```rust
impl TextField {
    pub fn from_str(s: &str) -> Self {
        let len = s.chars().count();
        TextField { value: s.to_string(), cursor: len, anchor: None }
    }

    pub fn len(&self) -> usize { self.value.chars().count() }
    pub fn is_empty(&self) -> bool { self.value.is_empty() }

    /// Selection as a half-open char range [start, end), if active and non-empty.
    pub fn selected_range(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        let (lo, hi) = if a <= self.cursor { (a, self.cursor) } else { (self.cursor, a) };
        if lo == hi { None } else { Some((lo, hi)) }
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.value.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(self.value.len())
    }

    fn delete_range(&mut self, lo: usize, hi: usize) {
        let blo = self.byte_at(lo);
        let bhi = self.byte_at(hi);
        self.value.replace_range(blo..bhi, "");
        self.cursor = lo;
        self.anchor = None;
    }

    fn delete_selection_if_any(&mut self) -> bool {
        if let Some((lo, hi)) = self.selected_range() {
            self.delete_range(lo, hi);
            true
        } else {
            self.anchor = None;
            false
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.delete_selection_if_any();
        let b = self.byte_at(self.cursor);
        self.value.insert(b, ch);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.delete_selection_if_any() { return; }
        if self.cursor > 0 {
            self.delete_range(self.cursor - 1, self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.delete_selection_if_any() { return; }
        if self.cursor < self.len() {
            self.delete_range(self.cursor, self.cursor + 1);
        }
    }

    /// dir: -1 left, +1 right. shift extends selection, else collapses it.
    pub fn move_cursor(&mut self, dir: i32, shift: bool) {
        if shift {
            if self.anchor.is_none() { self.anchor = Some(self.cursor); }
        } else {
            self.anchor = None;
        }
        if dir < 0 { self.cursor = self.cursor.saturating_sub(1); }
        else if dir > 0 { self.cursor = (self.cursor + 1).min(self.len()); }
    }

    pub fn move_home(&mut self, shift: bool) {
        if shift { if self.anchor.is_none() { self.anchor = Some(self.cursor); } } else { self.anchor = None; }
        self.cursor = 0;
    }

    pub fn move_end(&mut self, shift: bool) {
        if shift { if self.anchor.is_none() { self.anchor = Some(self.cursor); } } else { self.anchor = None; }
        self.cursor = self.len();
    }

    pub fn delete_word_left(&mut self) {
        if self.delete_selection_if_any() { return; }
        let chars: Vec<char> = self.value.chars().collect();
        let mut i = self.cursor;
        while i > 0 && chars[i - 1].is_whitespace() { i -= 1; }
        while i > 0 && !chars[i - 1].is_whitespace() { i -= 1; }
        self.delete_range(i, self.cursor);
    }

    pub fn set_cursor(&mut self, idx: usize) {
        self.cursor = idx.min(self.len());
        self.anchor = None;
    }

    pub fn begin_drag(&mut self, idx: usize) {
        let i = idx.min(self.len());
        self.cursor = i;
        self.anchor = Some(i);
    }

    pub fn extend_drag(&mut self, idx: usize) {
        self.cursor = idx.min(self.len());
    }
}
```

- [ ] **Step 4: Register + run tests**

In `crates/dd_ftp_app/src/lib.rs` add `mod text_field;` and `pub use text_field::TextField;`

Run: `cargo test -p dd_ftp_app text_field`
Expected: PASS (8 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/dd_ftp_app/src/text_field.rs crates/dd_ftp_app/src/lib.rs
git commit -m "feat(app): TextField cursor+selection model"
```

### Task 8: Route prompt input through TextField

**Files:**
- Modify: `crates/dd_ftp_app/src/state.rs` (`prompt_value`)
- Modify: `crates/dd_ftp_app/src/reducer.rs` (prompt arms)
- Modify: `crates/dd_ftp_ui/src/render.rs` (prompt render reads `.value`)
- Modify: `crates/dd_ftp_cli/src/main.rs` (any `prompt_value` reads)
- Test: `crates/dd_ftp_app/src/reducer.rs` (`#[cfg(test)]`)

- [ ] **Step 1: Write failing reducer test**

Add to `reducer.rs` a test module (or extend the existing one):

```rust
#[cfg(test)]
mod prompt_tests {
    use super::*;
    use crate::AppState;

    #[test]
    fn prompt_input_inserts_at_cursor() {
        let mut s = AppState::default();
        reduce(&mut s, Action::ShowCreatePrompt);
        reduce(&mut s, Action::PromptInput('a'));
        reduce(&mut s, Action::PromptInput('b'));
        assert_eq!(s.prompt_value.value, "ab");
        assert_eq!(s.prompt_value.cursor, 2);
    }
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p dd_ftp_app prompt_input_inserts_at_cursor`
Expected: FAIL — `prompt_value.value` not a field (still `String`).

- [ ] **Step 3: Change the type + reducer arms**

In `state.rs`: change `pub prompt_value: String,` to `pub prompt_value: TextField,` (add `use crate::TextField;`), and in `Default` change `prompt_value: String::new(),` to `prompt_value: TextField::default(),`.

In `reducer.rs` replace the four prompt-touching arms:
- `Action::PromptInput(ch) => { state.prompt_value.insert_char(ch); }`
- `Action::PromptBackspace => { state.prompt_value.backspace(); }`
- In `ShowCreatePrompt`/`ShowRenamePrompt`/`ShowDeletePrompt`/`ConfirmPrompt`/`CancelPrompt`, replace `state.prompt_value.clear();` with `state.prompt_value = TextField::default();`

- [ ] **Step 4: Fix readers**

In `render.rs:835` change `Span::styled(&app.prompt_value, ...)` to `Span::styled(&app.prompt_value.value, ...)`.
Search `main.rs` for `prompt_value` (the confirm path that reads the typed name) and change reads to `.value` / `.value.clone()` / `.value.as_str()` as the context requires.

Run: `cargo build` to surface every remaining `prompt_value` type mismatch and fix each by appending `.value`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p dd_ftp_app && cargo build -p dd_ftp_cli`
Expected: PASS + compiles.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(app): prompt_value backed by TextField"
```

### Task 9: Quick-connect active-field TextField + selection-aware input

**Files:**
- Modify: `crates/dd_ftp_app/src/state.rs` (add `qc_field: TextField` + sync helpers)
- Modify: `crates/dd_ftp_app/src/reducer.rs` (`QuickConnectInput`/`Backspace`/field-change sync)
- Test: `crates/dd_ftp_app/src/reducer.rs`

The 8 quick-connect values stay typed on `ConnectionInfo`. A single `qc_field: TextField` mirrors the **currently focused** field: hydrated on field change, flushed back to `ConnectionInfo` after each edit.

- [ ] **Step 1: Write failing test**

```rust
#[cfg(test)]
mod qc_tests {
    use super::*;
    use crate::{AppState, QuickConnectField};

    #[test]
    fn qc_field_change_hydrates_text_field() {
        let mut s = AppState::default();
        s.quick_connect.host = "example.com".into();
        s.quick_connect_field = QuickConnectField::Host;
        reduce(&mut s, Action::QuickConnectSyncField); // hydrate active field
        assert_eq!(s.qc_field.value, "example.com");
        assert_eq!(s.qc_field.cursor, 11);
    }

    #[test]
    fn qc_backspace_with_selection_clears_field() {
        let mut s = AppState::default();
        s.quick_connect.host = "abc".into();
        s.quick_connect_field = QuickConnectField::Host;
        reduce(&mut s, Action::QuickConnectSyncField);
        s.qc_field.anchor = Some(0);
        s.qc_field.cursor = 3; // whole value selected
        reduce(&mut s, Action::QuickConnectBackspace);
        assert_eq!(s.quick_connect.host, "");
    }
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p dd_ftp_app qc_field_change_hydrates`
Expected: FAIL — `qc_field` / `QuickConnectSyncField` missing.

- [ ] **Step 3: Add state field + sync helpers**

In `state.rs`: add `pub qc_field: TextField,` and `qc_field: TextField::default(),` in `Default`. Add two methods on `AppState`:

```rust
    pub fn qc_active_value(&self) -> String {
        use crate::QuickConnectField as F;
        match self.quick_connect_field {
            F::Name => self.quick_connect.name.clone(),
            F::Host => self.quick_connect.host.clone(),
            F::Port => self.quick_connect.port.to_string(),
            F::Username => self.quick_connect.username.clone(),
            F::Password => self.quick_connect.password.clone().unwrap_or_default(),
            F::PrivateKey => self.quick_connect.private_key.clone().unwrap_or_default(),
            F::Path => self.quick_connect.initial_path.clone(),
            F::Protocol => String::new(),
        }
    }

    pub fn qc_hydrate(&mut self) {
        self.qc_field = TextField::from_str(&self.qc_active_value());
    }

    pub fn qc_flush(&mut self) {
        use crate::QuickConnectField as F;
        let v = self.qc_field.value.clone();
        match self.quick_connect_field {
            F::Name => self.quick_connect.name = v,
            F::Host => self.quick_connect.host = v,
            F::Port => self.quick_connect.port = v.parse().unwrap_or(0),
            F::Username => self.quick_connect.username = v,
            F::Password => self.quick_connect.password = Some(v),
            F::PrivateKey => self.quick_connect.private_key = Some(v),
            F::Path => self.quick_connect.initial_path = v,
            F::Protocol => {}
        }
    }
```

- [ ] **Step 4: Add actions + reducer arms**

In `actions.rs` add: `QuickConnectSyncField`, `QuickConnectSetCursor(usize)`, `QuickConnectBeginSelect(usize)`, `QuickConnectExtendSelect(usize)`, `QuickConnectMoveCursor { dir: i32, shift: bool }`.

In `reducer.rs`:
- `Action::QuickConnectSyncField => { state.qc_hydrate(); }`
- Replace `QuickConnectInput(ch)` body with: filter Port to digits only, then `state.qc_field.insert_char(ch); state.qc_flush();`

```rust
        Action::QuickConnectInput(ch) => {
            if state.quick_connect_field == QuickConnectField::Port && !ch.is_ascii_digit() {
                // ignore non-digits in the port field
            } else {
                state.qc_field.insert_char(ch);
                state.qc_flush();
            }
        }
```

- Replace `QuickConnectBackspace` body with: `state.qc_field.backspace(); state.qc_flush();`
- `QuickConnectSetCursor(i) => { state.qc_field.set_cursor(i); }`
- `QuickConnectBeginSelect(i) => { state.qc_field.begin_drag(i); }`
- `QuickConnectExtendSelect(i) => { state.qc_field.extend_drag(i); }`
- `QuickConnectMoveCursor { dir, shift } => { state.qc_field.move_cursor(dir, shift); }`
- In `QuickConnectNextField`/`QuickConnectPrevField`, after changing the field, append `state.qc_hydrate();`
- In `ToggleQuickConnect` (when opening) and `QuickConnectSetFromBookmark`, append `state.qc_hydrate();`

The old `quick_connect_dirty_fields` "clear on first keystroke" behavior is now obsolete (the TextField shows real content and the user edits it directly). Remove the `quick_connect_dirty_fields` reads in these two arms; leave the struct field in place if other code references it, otherwise delete it and its `state.rs` declaration. Run `cargo build` to find references.

- [ ] **Step 5: Run tests + build**

Run: `cargo test -p dd_ftp_app && cargo build`
Expected: PASS + compiles.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(app): quick-connect fields backed by TextField"
```

### Task 10: Keyboard cursor editing in fields (CLI)

**Files:**
- Modify: `crates/dd_ftp_cli/src/main.rs` (quick-connect + prompt key handlers)

Wire arrows / Home / End / shift-select / Ctrl-W to the new actions and TextField methods.

- [ ] **Step 1: Quick-connect key handler**

Find the quick-connect key-handling block in `main.rs` (where `QuickConnectInput`/`QuickConnectBackspace`/`QuickConnectNextField` are dispatched). Add, before the existing `Char`/`Backspace` arms:

```rust
                        KeyCode::Left => {
                            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                            reduce(app, Action::QuickConnectMoveCursor { dir: -1, shift });
                        }
                        KeyCode::Right => {
                            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                            reduce(app, Action::QuickConnectMoveCursor { dir: 1, shift });
                        }
                        KeyCode::Home => reduce(app, Action::QuickConnectSetCursor(0)),
                        KeyCode::End => {
                            let len = app.qc_field.len();
                            reduce(app, Action::QuickConnectSetCursor(len));
                        }
                        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.qc_field.delete_word_left();
                            app.qc_flush();
                        }
```

- [ ] **Step 2: Prompt key handler**

Find the prompt key block (`PromptInput`/`PromptBackspace`/`ConfirmPrompt`). Add equivalent arms operating directly on `app.prompt_value` (a `TextField`):

```rust
                        KeyCode::Left => {
                            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                            app.prompt_value.move_cursor(-1, shift);
                        }
                        KeyCode::Right => {
                            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                            app.prompt_value.move_cursor(1, shift);
                        }
                        KeyCode::Home => app.prompt_value.move_home(false),
                        KeyCode::End => app.prompt_value.move_end(false),
                        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.prompt_value.delete_word_left();
                        }
```

- [ ] **Step 3: Build + manual verify**

Run: `cargo build -p dd_ftp_cli`. Then `cargo run -p dd_ftp_cli`, open quick connect, type in Host, press Left/Left, type → inserts mid-string. Shift+Left selects; typing replaces selection. Ctrl-W deletes a word. Same in a create-file prompt.

- [ ] **Step 4: Commit**

```bash
git add crates/dd_ftp_cli/src/main.rs
git commit -m "feat(cli): keyboard cursor editing in input fields"
```

### Task 11: Mouse click + drag selection in fields (CLI + render cursor)

**Files:**
- Modify: `crates/dd_ftp_cli/src/main.rs` (Down/Drag on `Region::Field`)
- Modify: `crates/dd_ftp_ui/src/render.rs` (render cursor + selection highlight in active field)

- [ ] **Step 1: Click + drag in the mouse Down/Drag arms**

In the `MouseEventKind::Down(Left)` branch's `hit_test` match, add a field arm. It must (a) move quick-connect focus to the clicked field, (b) sync, (c) set the cursor/anchor at the clicked char. For the prompt, operate on `prompt_value` directly:

```rust
                            Some(Region::Field(fid)) => {
                                if let Some(fr) = app_layout.fields.iter().find(|f| f.id == fid).copied() {
                                    match fid {
                                        FieldId::Prompt => {
                                            let len = app.prompt_value.len();
                                            let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                                            app.prompt_value.begin_drag(idx);
                                            drag_field = Some(fid);
                                        }
                                        _ => {
                                            if let Some(qf) = qc_field_for(fid) {
                                                app.quick_connect_field = qf;
                                                reduce(app, Action::QuickConnectSyncField);
                                                let len = app.qc_field.len();
                                                let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                                                reduce(app, Action::QuickConnectBeginSelect(idx));
                                                drag_field = Some(fid);
                                            }
                                        }
                                    }
                                }
                            }
```

Add a `let mut drag_field: Option<dd_ftp_ui::FieldId> = None;` next to `drag`. In the `MouseEventKind::Drag(Left)` branch add:

```rust
                        if let Some(fid) = drag_field {
                            if let Some(fr) = app_layout.fields.iter().find(|f| f.id == fid).copied() {
                                match fid {
                                    FieldId::Prompt => {
                                        let len = app.prompt_value.len();
                                        let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                                        app.prompt_value.extend_drag(idx);
                                    }
                                    _ => {
                                        let len = app.qc_field.len();
                                        let idx = dd_ftp_ui::char_index_at(&fr, mx, len);
                                        reduce(app, Action::QuickConnectExtendSelect(idx));
                                    }
                                }
                            }
                        }
```

In `MouseEventKind::Up(_)` add `drag_field = None;`.

Add the helper near `apply_scrollbar_drag`:

```rust
fn qc_field_for(fid: dd_ftp_ui::FieldId) -> Option<dd_ftp_app::QuickConnectField> {
    use dd_ftp_app::QuickConnectField as F;
    use dd_ftp_ui::FieldId::*;
    Some(match fid {
        QcName => F::Name, QcHost => F::Host, QcPort => F::Port,
        QcUsername => F::Username, QcPassword => F::Password,
        QcPrivateKey => F::PrivateKey, QcPath => F::Path,
        Prompt => return None,
    })
}
```

- [ ] **Step 2: Render selection highlight + cursor**

In `render.rs`, for the active quick-connect field and the prompt, render the value with the selected range styled with `t.selection` background (add token — see Step 3) and a cursor block at `cursor`. Replace the single value `Span` with a helper that splits the string into before-selection / selection / after-selection spans and inserts a cursor marker. Add near the top of `render.rs`:

```rust
fn render_field_line(tf: &dd_ftp_app::TextField, masked: bool, t: &Theme) -> Vec<Span<'static>> {
    let display: String = if masked { "•".repeat(tf.len()) } else { tf.value.clone() };
    let sel = tf.selected_range();
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, ch) in display.chars().enumerate() {
        let selected = sel.map(|(lo, hi)| i >= lo && i < hi).unwrap_or(false);
        let style = if selected {
            Style::default().fg(t.input_text_focus).bg(t.selection)
        } else {
            Style::default().fg(t.input_text_focus)
        };
        spans.push(Span::styled(ch.to_string(), style));
    }
    // cursor block at the end (cursor mid-string is a future refinement)
    spans.push(Span::styled("█", Style::default().fg(t.cursor).add_modifier(Modifier::RAPID_BLINK)));
    spans
}
```

One char per `Span` is simple and correct; batch contiguous same-style chars later only if it shows up in profiling. Use it for the active quick-connect field value (the focused row) and for the prompt input line (`render.rs:835`). Non-focused quick-connect rows keep their current plain rendering. (`masked = true` for the Password field.)

The cursor block is drawn at line end above. Drawing it at the actual `cursor` char index (so mid-string clicks show the caret in place) is a small follow-up: split the span loop at `tf.cursor` and insert the block there.

- [ ] **Step 3: Add `selection` theme token**

Per CLAUDE.md theming rule: add `selection` to `THEME_STRUCTURE_STANDARD.md`, then a `pub selection: Color` field + default in `crates/dd_ftp_ui/src/theme.rs` (a muted blue, e.g. matching `scrollbar_hover` or a dedicated value), then consume it in `render_field_line`. Do not hardcode the color.

- [ ] **Step 4: Build + manual verify**

Run: `cargo build && cargo clippy --workspace --all-targets`. Then `cargo run -p dd_ftp_cli`: open quick connect, click mid-Host → cursor lands there; click-drag across Host → range highlights with the selection color; press Backspace → selection deleted; type → selection replaced. Repeat in a create-file prompt. Password field shows masked bullets but selection/cursor still track.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: mouse click+drag text selection in input fields"
```

### Task 12: Final pass — fmt, clippy, full test run

- [ ] **Step 1: Format + lint + test**

Run:
```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test
```
Expected: no fmt diff after commit, clippy clean, all tests pass.

- [ ] **Step 2: Update README controls section**

Add a Mouse section to `README.md` documenting: wheel scroll (lists/queue/help), click to focus+select, double-click to enter dir / load bookmark, scrollbar drag, click+drag text selection in fields, and the new keyboard editing keys (arrows/Home/End/Shift-select/Ctrl-W).

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "docs: document mouse + field-editing controls"
```

---

## Self-Review notes (carried into execution)

- **Render geometry is the fragile part.** `text_x` / `label_width` / per-row rects in Task 3/4 must be read from the *actual* render code at edit time, not assumed — the plan gives the shape; the exact column offsets come from the existing label spans.
- **`render_field_line` style-coalescing** is left per-char for clarity; if it shows measurable overhead on long fields, batch contiguous same-style chars. Not required for correctness. Cursor renders at line end in the given code; mid-string caret is a noted follow-up.
- **`quick_connect_dirty_fields`** removal (Task 9 Step 4) may touch the render path that styles "dirty" fields — `cargo build` will flag every reference; resolve each.
- **Queue/help scroll bounds**: `apply_scrollbar_drag` approximates totals; the render-side `render_scrollbar` clamp already prevents overscroll visually, so an over-large `*_scroll` is harmless (clamped on draw).
```
