# ldnddev TUI Visual Standard

**Single source of truth for consistent look, feel, and structure across all ldnddev terminal user interface applications.**

This document incorporates and supersedes the previous separate guides (THEME_STRUCTURE_STANDARD.md for core theming, plus the now-deleted HEADER_FOOTER_GUIDE.md and SOURCE_PANEL_GUIDE.md).

All new ldnddev TUI apps (e.g. dd_dotstore, dd_ftp, etc.) **must** follow this standard so users experience a familiar, polished, and cohesive environment.

## How to Use This Standard When Starting a New App

1. Copy `LDNDDEV_TUI_VISUAL_STANDARD.md` (and a sample `dd_*_theme.yml`) into your new project's repository as the visual contract.

2. Pick your app's name (e.g. `dd_ftp`) and consistently use it for theme files, titles, etc.

3. Implement the shell layout first: fixed 3-line header with random tagline (customizable via `header_quotes` in theme), 1-line adaptive footer, using `app_shell` and `active_border` from the theme.

4. Implement the Source panel (or equivalent folder navigation) using the exact `Node`/`NodeKind` model, `build_tree` + `flatten_visible` logic (with Unicode tree prefixes), rendering structure (checkbox + icon + mode + name + badges), title with counts, and full keyboard + mouse interaction (zones, shift-range, scrollbar drag, double-click, etc.).

5. Load themes exactly as specified (local → global → defaults) and validate `version: 1`.

6. Map all UI elements to the canonical theme tokens (do not invent new ones).

7. Port the header tagline randomization (supporting `header_quotes` override from theme) and width-adaptive key hints.

8. Populate F1 Help with the full key + mouse reference, and F2 Credits with theme source/status.

9. Test on both narrow (<80 cols) and wide terminals; verify no overflow, good density, and consistent behavior.

10. If you discover improvements or new patterns, propose updates back into this master document first.

---

## Goals

- One shared visual language and interaction model
- Predictable theming with the same tokens meaning the same thing everywhere
- Local per-project overrides + global fallback
- Easy to implement and maintain (Ratatui + crossterm today, portable to future frameworks)
- Professional yet playful personality (witty taglines, clean minimal chrome, excellent mouse + keyboard support)
- Maximum information density without clutter, especially on smaller terminals

---

## 1. Theme System

### Lookup Order (every app must follow exactly)
1. `./<PROJECT_NAME>_theme.yml` (local override)
2. `~/.config/ldnddev/<PROJECT_NAME>_theme.yml` (global)
3. Built-in defaults inside the app

### Required Schema Version
All theme files **must** declare:
```yaml
version: 1
```

Apps must validate this on load and fall back gracefully with a visible warning.

### Canonical Color Tokens

```yaml
colors:
  base_background: "#0F1114"
  body_background: "#2A2D31"
  modal_background: "#1C1E21"

  text_primary: "#F5F6F7"
  text_secondary: "#9EA3AA"
  text_labels: "#FFAF46"
  text_active_focus: "#64B4F5"
  modal_labels: "#64B4F5"
  modal_text: "#F5F6F7"

  selected_background: "#0F1114"

  border_default: "#F5F6F7"
  border_active: "#64B4F5"
  scrollbar: "#FFA087"
  scrollbar_hover: "#64B4F5"

  input_border_default: "#F5F6F7"
  input_border_focus: "#64B4F5"
  input_text_default: "#F5F6F7"
  input_text_focus: "#64B4F5"
  cursor: "#64B4F5"

  success: "#82e0aa"
  warning: "#f5c469"
  error: "#e57373"
  info: "#5dade2"

  folders: "#64B4F5"
  files: "#FFAF46"
  links: "#FFA087"

### Header Quotes (optional, top-level)
```yaml
header_quotes:
  - "Your first witty tagline."
  - "Second one here."
  # ... up to as many as you like
```

- If present and non-empty, these replace the built-in defaults for the rotating header banner.
- Strings should be short (single line).
- If omitted, the app's built-in defaults are used (see app source for current list).

### Strict Mapping Rules

**Backgrounds**
- `base_background` → entire app shell, header, footer
- `body_background` → main content panes (lists, trees, tables)
- `modal_background` → every modal/dialog

**Text**
- `text_primary` → primary content text
- `text_secondary` → muted / secondary text
- `text_labels` → default labels
- `text_active_focus` → active / focused / selected labels
- `modal_labels` / `modal_text` → modal-specific

**Selections & Chrome**
- `selected_background` → highlighted row background (use with `text_active_focus` for text)
- `border_default` → normal panel borders
- `border_active` → the "active" panel's border (usually the one with focus)

**Inputs & Scrollbars**
- Full set of input tokens (border + text + focus + cursor) must be used for any editable field
- `scrollbar` and `scrollbar_hover` for all scrollbars

**Semantic & Domain Colors**
- `success`, `warning`, `error`, `info` for toasts, status, alerts
- `folders`, `files`, `links` for tree / file listing semantics

**Never** hard-code colors after the theme is loaded. Always pull from the `Theme` struct.

### Theme Status & Credits
- Every app must expose the active theme source (`local` / `global` / `default`) and schema version.
- Show theme health at startup (in footer or toast).
- Full details must appear in the F2 Credits modal.

See the original `THEME_STRUCTURE_STANDARD.md` for the complete validation checklist and anti-patterns.

---

## 2. Overall Layout & Shell

Every app uses this exact vertical structure in its main draw function:

```rust
let outer = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Length(3),   // HEADER (fixed)
        Constraint::Min(0),      // Main content area
        Constraint::Length(1),   // FOOTER (fixed, decluttered)
    ])
    .split(f.area());
```

- Full-screen `Block` using `app_shell` style as the base layer.
- **Header** always occupies the top 3 lines.
- **Footer** always occupies the bottom 1 line.
- The middle area is where your primary panels live (50/50 Source + secondary panel is the recommended default for browser-style apps).

**Do not** make header/footer heights dynamic. Hard-code these values for a consistent "ldnddev" shell across all apps.

---

## 3. Header Component

### Visual & Structure
- Bordered `Block` with:
  - `.title("dd_xxx")` (project name, e.g. "dd_dotstore", "dd_ftp")
  - `.borders(Borders::ALL)`
  - `.border_style(theme.active_border)`
  - `.style(theme.app_shell)`
- Content: a single line of text (`state.header_copy`)
- Always 3 lines tall (including borders).

### Content (Taglines)
The taglines are randomized at startup from a list of strings.

By default, dd_dotstore uses these 5 built-in ones (feel free to customize per-app):

- "Don't Fear the . (Dot) - Tame It."
- ". (Dot) file Domination done right."
- etc.

**Customization:** Users can override the list by adding a `header_quotes:` section (list of strings) to their `dd_*_theme.yml` file. If omitted, the app's built-in defaults are used.

Example in theme file:
```yaml
header_quotes:
  - "Your custom quote here."
  - "Another one for variety."
```

The randomization logic remains the same (time-based seed XOR PID for reproducibility per run but different each launch).

Taglines should be fun, short, one-line, and match the app's personality.

### Theming
- Whole header uses `app_shell` (base_background + text_primary).
- Title border uses `active_border`.

### Behavior
- Purely decorative / branding.
- No mouse or keyboard interaction.
- Only changes on application restart.

---

## 4. Footer / Status Bar Component

### Visual & Structure
- Borderless `Paragraph` using `.style(theme.app_shell)`.
- Fixed 1 line high.
- Content is a single, width-adaptive line of key hints.

### Content (Adaptive Key Hints)
```rust
let keys = if area.width < 75 {
    "F1:Help  q:Quit  j/k:Nav  Spc:Sel  s:Apply  x:Rem  /:Filter"
} else if area.width < 110 {
    "F1: Help   /: Search   Space: Select   m/M: Link/Copy   s: Apply   x: Remove   Q: Exit"
} else {
    "F1: Help   /: Search   Space: Select   m/M: Link/Copy   s: Apply   x: Remove   Q: Exit   (mouse: click/scroll/drag)"
};
```

**Rules**
- Always start with `F1:Help`.
- Use very terse abbreviations on narrow terminals.
- Only show the mouse reminder on very wide terminals.
- The full authoritative list of keys (including mouse) lives in the F1 Help modal.

**Theme status / health** is **no longer shown persistently** in the footer (removed for declutter). It appears in:
- F2 Credits modal
- Startup (initial message or toast on warnings)

---

## 5. Source / Folder Navigation Panel (Primary Content Component)

This is the reusable "browser" view that most file-oriented ldnddev apps will need.

### Data Model (keep this shape consistent)
```rust
pub struct Node {
    pub name: String,
    pub path: PathBuf,           // relative to root
    pub kind: NodeKind,
    pub selected: bool,
    pub action_mode: ActionMode, // can be repurposed
    pub symlink_status: SymlinkStatus, // or your domain status
}

pub enum NodeKind {
    File { dest: Option<PathBuf> },
    Folder { children: Vec<Node>, expanded: bool, dest: Option<PathBuf> },
}
```

### Building & Flattening
- `build_tree(root, ignores)` — recursive, dirs first, alpha-sorted, respects ignore list, starts collapsed.
- `flatten_visible(state)` — produces the display list with proper Unicode tree prefixes (`├─`, `└─`, `│  `), filter-aware, updates `list_state` selection.

Use the exact tree prefix logic and visible-child filtering from `src/tree.rs`.

### Rendering
- Stateful `List` with `highlight_style(theme.selected)`.
- Per-row structure (left to right):
  1. Checkbox: `[ ] ` or `[✓] `
  2. Status icon (✓ ✗ ? or app-specific + subtree badges ◌ ●)
  3. Mode label `[LINK] ` / `[COPY] ` (or your equivalent)
  4. Name (tree-prefixed) with folder/file color + optional ● badge
- Dynamic title: `Source [N selected / M]` or `Source (filter: xxx)  [N selected / M]`
- Bordered block using `active_border` + `body` style.
- Right-edge `Scrollbar` (VerticalRight, no symbols) when content overflows, driven by `list_state.offset()`.

**Always** capture `state.source_area = area;` at the start of the draw function for mouse hit-testing.

### Interaction (both keyboard & mouse)
**Keyboard (core set that must be supported)**
- j/k / arrows: move selection
- Space: toggle checkbox
- h/l / arrows: expand/collapse folder
- Enter: activate (usually opens a destination / action browser)
- m/M: toggle action mode (single or bulk)
- / : open filter
- G / g (Ctrl-g top): jumps
- r : reload

**Mouse (full support required for consistency)**
- Capture `source_area` every frame.
- Checkbox zone (left ~4 cols): toggle select
- Tree connector / glyph zone on folders (roughly cols 12-18): toggle expand
- Name area + double-click (420ms exact position, name_part): activate
- Wheel (only over source area): scroll with Shift = faster
- Right-edge scrollbar: drag or click to scroll (proportional, works even if mouse leaves the column while button held)
- Shift+click: range multi-select
- Maintain `last_mouse_click_pos` for double-click detection and `scrollbar_dragging` flag.

See the detailed zone calculations and helpers in `src/inputs.rs` (`hit_test_source_row`, `is_folder_glyph`, `update_source_scrollbar`, etc.).

### Theming for Source Panel
Use these tokens (in addition to the shell ones):
- `folder` / `file` for names
- `valid` / `broken` / `highlight` for status icons
- `links` or `valid` for action mode labels
- `secondary` for ● / ◌ badges
- `scrollbar` for the tree scrollbar
- `normal` for checkbox / default icon
- `selected` for the highlighted row

Subtree badges ("●" for has configured descendants, "◌ " for folders containing them) are strongly recommended for information density.

---

## 6. Destinations / Secondary Panel (Recommended Companion)

When your app has a "Source" tree, it almost always benefits from a right-hand "Destinations" / "Applied" / "Remote" panel (50/50 split is the current standard).

- Walk the same tree to collect items that have a `dest`.
- Render a flat list of "• MODE src → dest"
- Capture `status_area`, use `status_list_state`.
- Provide the same mouse support: wheel, scrollbar drag, click-to-jump (with `expand_ancestors` on the source side).
- Title should show count: `Destinations (N)`

This pairing is what gives dd_dotstore its characteristic two-pane browser feel.

---

## 7. Common Shell Elements

- **F1 Help modal**: Must contain the full key list + complete mouse documentation + any app-specific notes.
- **F2 Credits modal**: Must show `Theme source: local/global/default` and the full `Theme status` message.
- **Toasts**: Use the four semantic colors (`success`, `warning`, `error`, `info`). Bottom-right, auto-dismiss after ~5s.
- **Modals**: Centered, use `modal_background` + `modal_text` / `modal_labels`. Clear the background underneath.
- **Input fields** (when present): Must use the full set of `input_*` tokens.

---

## 8. Implementation & Maintenance Rules

- Hard-code the shell heights (header 3, footer 1).
- Always capture `*_area` rects for every interactive panel at draw time (required for mouse).
- Use the exact adaptive key-hint logic in the footer (or an equivalent that follows the same "short on narrow, full on wide" spirit).
- Never put long persistent status text in the footer.
- Load and validate the theme exactly as described.
- Keep the Source panel data model and interaction zones as close as possible to the spec (customize only the icon/status column and action modes).
- Update this master document first whenever you add a new shared visual or interaction pattern.

---

## 9. Versioning

This standard is currently at **v1**. Bump the version here and in the individual theme schema when breaking changes are introduced.

---

**Follow this document and your apps will feel like a family, not a collection of unrelated tools.**

For the most up-to-date concrete code, always look at the current implementation inside dd_dotstore (`src/ui.rs`, `src/tree.rs`, `src/state.rs`, `src/inputs.rs`, `src/app.rs`) and the sample theme files.

If a new UI element appears that needs a color or layout convention, propose the addition here first before implementing it in a single app.