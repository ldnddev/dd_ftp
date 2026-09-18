use ldnddev_theme::{
    load_from_str, parse_hex_input, render_yaml, save_theme, EditorKey, EditorOutcome, Palette,
    ParseMode, ThemeEditor, ThemeSaveTarget, ThemeSource, COLOR_FIELDS, EXTRA_MODAL_HEADER,
    EXTRA_SELECTION,
};

fn full_yaml() -> String {
    let mut body = String::from("version: 1\nheader_quotes:\n  - \"Hello\"\ncolors:\n");
    for field in COLOR_FIELDS {
        body.push_str(&format!("  {}: \"#010203\"\n", field.key));
    }
    body
}

#[test]
fn strict_requires_every_canonical_key() {
    let err = load_from_str(
        "version: 1\ncolors:\n  base_background: \"#010203\"\n",
        ParseMode::Strict,
        ThemeSource::Local,
    )
    .expect_err("missing keys");
    assert!(err.to_string().contains("Missing required theme color"));
}

#[test]
fn lenient_fills_missing_keys() {
    let palette = load_from_str(
        "version: 1\ncolors:\n  base_background: \"#AABBCC\"\n",
        ParseMode::Lenient,
        ThemeSource::Global,
    )
    .expect("lenient");
    assert_eq!(palette.get("base_background").unwrap().to_hex(), "#AABBCC");
    assert!(palette.get("folders").is_some());
    assert_eq!(palette.source, ThemeSource::Global);
}

#[test]
fn strict_round_trip() {
    let palette = load_from_str(&full_yaml(), ParseMode::Strict, ThemeSource::Local).expect("load");
    assert_eq!(palette.header_quotes, vec!["Hello".to_string()]);
    let yaml = render_yaml(&palette, COLOR_FIELDS);
    let again = load_from_str(&yaml, ParseMode::Strict, ThemeSource::Local).expect("reload");
    assert_eq!(again.get("base_background").unwrap().to_hex(), "#010203");
}

#[test]
fn editor_defaults_to_global_and_esc_reverts() {
    let mut editor = ThemeEditor::new(Palette::builtin(), &[]);
    assert_eq!(editor.save_target, ThemeSaveTarget::Global);
    let original = editor.palette.get("base_background").unwrap();
    let changed = editor.handle(EditorKey::Right, false);
    assert_eq!(changed, EditorOutcome::PaletteChanged);
    assert_ne!(editor.palette.get("base_background").unwrap(), original);
    let closed = editor.handle(EditorKey::Esc, false);
    assert_eq!(closed, EditorOutcome::Closed { reverted: true });
    assert_eq!(editor.palette.get("base_background").unwrap(), original);
}

#[test]
fn extra_fields_appear_and_save() {
    let editor = ThemeEditor::new(Palette::builtin(), &[EXTRA_MODAL_HEADER, EXTRA_SELECTION]);
    assert!(editor.fields.iter().any(|f| f.key == "modal_header"));
    assert!(editor.palette.get("selection").is_some());
    let dir = tempfile::tempdir().expect("tmp");
    let path = save_theme(
        &editor.palette,
        dir.path(),
        "dd_test_theme.yml",
        ThemeSaveTarget::Local,
        None,
        &editor.fields,
    )
    .expect("save");
    let yaml = std::fs::read_to_string(path).expect("read");
    assert!(yaml.contains("modal_header:"));
    assert!(yaml.contains("selection:"));
}

#[test]
fn hex_parse_and_commit() {
    assert_eq!(parse_hex_input("aabbcc").unwrap().to_hex(), "#AABBCC");
    let mut editor = ThemeEditor::new(Palette::builtin(), &[]);
    assert_eq!(editor.handle(EditorKey::Enter, false), EditorOutcome::None);
    editor.hex_draft.clear();
    for c in ['#', '1', '1', '2', '2', '3', '3'] {
        editor.handle(EditorKey::Char(c), false);
    }
    assert_eq!(
        editor.handle(EditorKey::Enter, false),
        EditorOutcome::PaletteChanged
    );
    assert_eq!(
        editor.palette.get("base_background").unwrap().to_hex(),
        "#112233"
    );
}
