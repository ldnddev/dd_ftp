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

1. `./PROJECT_NAME_theme.yml` (project-local override, example file name './dd_ftp_theme.yml')
2. `~/.config/ldnddev/PROJECT_NAME_theme.yml` (global default, example file name './dd_ftp_theme.yml')
3. Built-in application defaults

> If adapting for another app name, keep the same schema and only rename file prefix if needed.

---

## Canonical Theme Schema (YAML)

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

### 2) Text colors

- `text_primary`: "#F5F6F7"
    -  Primary text
- `text_secondary`
    - Secondary/muted text
- `text_labels`
    - Default state for labels
- `text_active_focus`
    - Active/selected/focused state for labels
- `modal_labels`
    - Popup/modal Label text
- `modal_text`
    - Popup/modal Primary text

### 3) Selections

- `selected_background`
    - Active/selected/focused row background

### 4) Borders (1px-equivalent in TUI)

- `border_default`
    -  Default borders
- `border_active`
    - Active section highlight

### 5) Input Fields (all text-entry fields)

Each editable field must have a bordered input box:  
- `input_border_default`
    - Default input border
- `input_border_focus`
    - Active/selected/focused input border
- `input_text_default` 
    - Default input border
- `input_text_focus` 
    - Active/selected/focused input border
- `cursor`
    - Cursor color

### 6) Scrollbars

- `scrollbar`
    - All scrollbars (pane + modal)
- `scrollbar_hover`
    - ALL scrollbar hover color (pane + modal)

### 7) Semantic colors
- `success`
    - Success toast messages
- `warning`
    - Warning toast messages
- `error`
    - Error toast messages
- `info`
    - Info toast messages
  
### 8) folder / file / link colors
- `folders`
    - Default folder color for text (if needed)
- `files`
    - Default file color for text (if needed)
- `links`
    - Default symlink color for text (if needed)

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

If a new UI element needs color mapping and no token exists, add a token here first, then implement it in code. Do not invent app-specific one-off tokens unless they are promoted into this standard.
