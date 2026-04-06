# ldnddev TUI Theme Structure Standard

Use this document as the single source of truth for theming all ldnddev TUI apps.

---

## Goals

- One shared theme token structure across apps
- Predictable visual mapping (same token = same UI intent)
- Local-per-project override + global fallback
- Easy portability across Rust TUIs (ratatui) and future frameworks

---

## File Name + Lookup Order

Every app should load theme files in this order:

1. `./dd_ftp_theme.yml` (project-local override)
2. `~/.config/ldnddev/dd_ftp_theme.yml` (global default)
3. Built-in application defaults

> If adapting for another app name, keep the same schema and only rename file prefix if needed.

---

## Canonical Theme Schema (YAML)

```yaml
colors:
  # Core backgrounds
  base_background: "#0f1114"
  body_background: "#1a1c1f"
  modal_background: "#2a2d31"

  # Core text
  text_primary: "#f5f6f7"
  text_secondary: "#9ea3aa"

  # Labels / headings
  text_labels: "#9ea3aa"
  text_labels_active: "#6ec8ff"
  text_active_focus: "#6ec8ff"

  # Modal-specific text
  modal_labels: "#f5f6f7"
  modal_text: "#f5f6f7"

  # Selection + borders
  selected_background: "#2a2d31"
  border_default: "#2a2d31"
  border_active: "#6ec8ff"

  # Input fields
  input_border_default: "#5ab4f5"
  input_border_focus: "#8cc8ff"
  input_text_default: "#5ab4f5"
  input_text_focus: "#8cc8ff"

  # Scrollbars
  scroll_bars: "#2a2d31"

  # Semantic state colors
  success: "#82e0aa"
  warning: "#f5c469"
  error: "#e57373"
  info: "#5dade2"
```

---

## Required Mapping Rules

These mappings should stay consistent across all apps.

### 1) Backgrounds

- `base_background`
  - Entire app shell/background
  - Header + footer background
- `body_background`
  - Main content panes (lists, tables, queue panels, etc.)
- `modal_background`
  - Every modal/dialog surface

### 2) Borders (1px-equivalent in TUI)

- **Default border**: `border_default`
- **Focused/active border**: `border_active`

### 3) Pane/Section Labels

- Default pane labels (`[1] Local`, `[2] Remote`, etc.): `text_labels`
- Active/focused pane label: `text_active_focus`

### 4) Modal Typography

- Modal title/label text: `modal_labels`
- Modal body/default text: `modal_text`

### 5) Input Fields (all text-entry fields)

Each editable field must have a bordered input box:

- default border: `input_border_default`
- focused border: `input_border_focus`
- default input text: `input_text_default`
- focused input text: `input_text_focus`
- input field labels: `text_labels_active`

### 6) Selection States

- Selected row/item background: `selected_background`

### 7) Scrollbars

- All scrollbars (pane + modal): `scroll_bars`

### 8) Semantic States

- Success states/messages: `success`
- Warning states/messages: `warning`
- Error states/messages: `error`
- Informational/progress states: `info`
- Secondary/meta text (helper/counters/subtext): `text_secondary`

---

## Optional Runtime UX Conventions

Recommended for all apps:

- Theme source indicator available in debug overlay:
  - `local`, `global`, or `default`
- Key toggle for theme debug overlay (e.g. `F2`)
- Theme health/status shown at startup

---

## Validation Checklist (Per App)

Before shipping any TUI app:

1. ✅ Load from local theme path
2. ✅ Fall back to global theme path
3. ✅ Fall back to built-in defaults
4. ✅ Confirm all schema keys parse correctly
5. ✅ Verify each token is visibly mapped somewhere
6. ✅ Ensure no hardcoded fallback colors override loaded theme unintentionally
7. ✅ Confirm active/focus states use active tokens (not defaults)

---

## Anti-Patterns to Avoid

- Hardcoding colors directly in render paths after theme is loaded
- Using one token for unrelated intents (e.g. using `warning` as body text)
- Missing focus-state mapping for borders/labels/inputs
- Inconsistent semantics between apps (e.g. `success` meaning error in one app)

---

## Versioning Recommendation

Add a small header field in future theme files:

```yaml
version: 1
```

Then enforce schema compatibility in loaders as apps evolve.

---

## Maintainer Note

If a new UI element needs color mapping and no token exists, add a token here first,
then implement it in code. Do not invent app-specific one-off tokens unless they are
promoted into this standard.
