# Full Mouse Support — Design

**Date:** 2026-06-12
**Status:** Approved for planning
**Crates touched:** `dd_ftp_app` (state/actions/reducer), `dd_ftp_ui` (render, layout map), `dd_ftp_cli` (event loop, mouse translation)

## Goal

Add complete mouse interaction to the TUI: wheel scroll, click, drag, and in-field
text selection. Mouse capture is already enabled (`main.rs:31`); today only
`MouseEventKind::Moved` is consumed (for scrollbar hover). Everything else
(click / wheel / drag) is captured but discarded (`main.rs:599-603`).

## Surfaces

| # | Surface | Interactions |
|---|---------|--------------|
| A | File lists (local / remote) | wheel = move selection by N (viewport follows); single click = focus pane + select row; double-click on dir = enter directory |
| B | Queue / Help panels | wheel = scroll (`queue_scroll` / `help_scroll`) |
| C | Scrollbars | click/drag thumb = scroll the owning region |
| D | Input fields (quick-connect, prompt) | click = position cursor; drag = select range; selection + type/backspace = replace; keyboard cursor editing (arrows / Home / End / shift-select / word-delete) |
| E | Modals / controls | bookmarks list row click = select (double = connect); protocol toggle click = cycle; prompt confirm/cancel hit areas |

## Architecture

### 1. Geometry via emitted LayoutMap (decision #3)

`render` computes all Rects today and throws them away. Change it to **return** a
`LayoutMap` describing the clickable regions of the frame it just drew. The CLI
caches the latest map; each incoming mouse event is hit-tested against it.

```rust
// dd_ftp_ui
pub struct LayoutMap {
    pub local_list: Rect,
    pub remote_list: Rect,
    pub queue: Rect,
    pub help: Option<Rect>,
    pub local_scrollbar: Rect,
    pub remote_scrollbar: Rect,
    pub queue_scrollbar: Rect,
    pub help_scrollbar: Option<Rect>,
    pub fields: Vec<FieldRegion>,   // quick-connect + prompt input boxes
    pub controls: Vec<ControlRegion>, // protocol toggle, confirm/cancel, bookmark rows
}

pub struct FieldRegion { pub id: FieldId, pub area: Rect, pub text_x: u16 }
pub struct ControlRegion { pub id: ControlId, pub area: Rect }

pub fn render(frame: &mut Frame, app: &AppState) -> LayoutMap;
```

`reduce` stays pure — it never sees a Rect. Hit-testing and event→action
translation live entirely in the CLI. Geometry has a single source (render), so
no drift. One-frame staleness is invisible: render runs every loop tick before
the event read.

`hit_test(map, x, y) -> Option<Region>` is a pure helper in `dd_ftp_ui`, unit-testable.

### 2. Text-field model (D)

New reusable struct in `dd_ftp_app`:

```rust
pub struct TextField {
    pub value: String,            // chars
    pub cursor: usize,            // char index, 0..=len
    pub anchor: Option<usize>,    // Some => active selection [min,max)
}
```

Operations (all pure methods): `insert_char`, `backspace`, `delete`,
`move_left/right` (with `shift: bool` to extend or collapse selection),
`move_home/end`, `delete_word_left`, `selected_range`, `set_cursor(idx)`,
`begin_drag(idx)` / `extend_drag(idx)`, `replace_selection(str)`. Typing or
backspace with an active selection replaces/removes the range first.

**Integration:** the currently-edited field is held as a `TextField`. For
quick-connect, the active field's string hydrates a `TextField` on focus change
and flushes back to `ConnectionInfo` on edit (the 8 fields stay typed on
`ConnectionInfo`; only the focused one carries cursor/selection). `prompt_value`
becomes a `TextField`. Password field renders masked but keeps real cursor
positions.

Mouse→char-index mapping: `char_index = (click_x - field.text_x)` clamped to
`0..=value.len()`, accounting for the password mask width. Provided by the
`FieldRegion.text_x` in the LayoutMap.

### 3. Event translation (CLI)

`main.rs` `Event::Mouse(mouse)` arm replaces the current Moved-only stub. A small
`MouseState` struct in the CLI tracks drag and double-click:

```rust
struct MouseState {
    drag_origin: Option<(MouseButton, u16, u16, DragKind)>, // what a down started
    last_click: Option<(u16, u16, Instant)>,                // for double-click
}
enum DragKind { TextSelect(FieldId), Scrollbar(Region), None }
```

Flow per event kind:
- `ScrollUp/Down` → look up region under cursor in `LayoutMap`; dispatch the
  matching scroll action (list = SelectUp/Down ×3; queue/help = scroll action).
- `Down(Left)` → hit-test. List row → `FocusPane` + select index; check
  double-click (Instant delta < 300ms + same cell) → navigate-into. Field → set
  cursor at char index, begin selection anchor, record `DragKind::TextSelect`.
  Scrollbar → record `DragKind::Scrollbar`, jump scroll to position. Control →
  activate.
- `Drag(Left)` → if `TextSelect`, extend field selection to char index under
  cursor; if `Scrollbar`, scroll region proportional to y.
- `Up` → clear `drag_origin`.
- `Moved` → keep existing `mouse_pos` hover behavior.

Timing (`Instant`) and drag state are CLI-local — never enter `reduce`.

### 4. New actions (dd_ftp_app)

- Field editing routed through new actions so `reduce` stays the mutation point:
  `FieldSetCursor { id, idx }`, `FieldBeginSelect { id, idx }`,
  `FieldExtendSelect { id, idx }`, `FieldMoveCursor { dir, extend }`,
  `FieldDeleteSelection`. Existing `QuickConnectInput`/`Backspace` and
  `PromptInput`/`Backspace` are reworked to operate on the active `TextField`
  (selection-aware).
- Region scroll reuses existing `SelectUp/Down`, `queue_scroll`, `help_scroll`
  paths where possible; add `ScrollQueue(delta)` / `ScrollHelp(delta)` /
  `ScrollListBy { pane, delta }` if the current per-key handlers are awkward to reuse.

## Data flow

```
crossterm Event::Mouse
  └─ CLI hit_test(cached LayoutMap, x, y)  [pure, dd_ftp_ui]
       └─ CLI MouseState updates drag/double-click  [Instant — CLI only]
            └─ reduce(&mut state, Action::…)  [pure]
                 └─ next render(frame,&state) -> LayoutMap  [cached for next event]
```

## Error / edge handling

- Click outside any region → no-op.
- Click in a list row past the last entry → no selection change.
- Drag that leaves the field → clamp char index to field bounds.
- Wheel over a modal scrolls the modal, not the list underneath (modal regions
  win in hit-test ordering; `any_modal_open()` already exists).
- Empty list → wheel/click no-op.
- Double-click threshold 300ms, same terminal cell required.

## Testing

- `TextField` unit tests: insert/backspace/delete, selection replace, word-delete,
  shift-select cursor moves, clamp at bounds, masked-field cursor math.
- `hit_test` unit tests: point→region for each surface incl. modal-over-list
  precedence and out-of-bounds.
- `char_index_at(field, x)` boundary tests (start, mid, end, past-end, masked).
- Reducer tests for the new field actions (selection-aware input/backspace).
- Manual: scroll/click/drag pass across each surface (no automated TUI harness).

## Staging (ship order, each independently testable)

1. **LayoutMap + hit_test** infra (render returns map, CLI caches, pure tests). No behavior change yet.
2. **B + A-scroll**: wheel scroll for lists, queue, help.
3. **A-click + E**: click-to-focus/select, double-click enter, bookmark/control clicks.
4. **C**: scrollbar click/drag.
5. **D**: `TextField` model + field actions + click/drag selection + keyboard cursor editing.

Each stage is a reviewable commit. Order puts the lowest-risk infra first and the
biggest single feature (D) last.

## Out of scope (YAGNI)

- Native terminal text selection / OS clipboard copy (conflicts with mouse
  capture; user wants in-app field selection, not native copy).
- Drag-to-multi-select file rows (single select + double-click only).
- Drag-and-drop file transfer between panes.
- Right-click context menus.
