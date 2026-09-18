# ldnddev_theme

Shared YAML theme tokens and live color-editor state for ldnddev TUI apps.

The crate does **not** depend on ratatui or crossterm. Convert colors with:

```rust
ratatui::style::Color::Rgb(rgb.r, rgb.g, rgb.b)
```

## Lookup

1. `./<app>_theme.yml` (local)
2. `$XDG_CONFIG_HOME/ldnddev/<app>_theme.yml` (global)
3. Built-in defaults

## Editor

`ThemeEditor::new(palette, extra_fields)` — save target defaults to **global**.
`handle(EditorKey, shift)` returns `PaletteChanged`, `RequestSave`, or `Closed`.
`save_theme(...)` writes schema `version: 1` YAML and keeps `header_quotes`.
