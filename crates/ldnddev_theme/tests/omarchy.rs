//! Omarchy colors.toml fixtures are copied from stock themes (MIT).
//! https://github.com/basecamp/omarchy

use ldnddev_theme::{
    apply_omarchy, colors_from_str, contrast_ratio, discover, load_from_str, map_palette, mix,
    render_yaml, EditorKey, OmarchyChoice, OmarchyPaths, Palette, ParseMode, PickerOutcome,
    ThemeOrigin, ThemePicker, ThemeSource, COLOR_FIELDS, EXTRA_MODAL_HEADER,
};
use std::fs;
use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/omarchy")
}

fn tokyo() -> String {
    fs::read_to_string(fixtures().join("tokyo-night/colors.toml")).expect("tokyo-night")
}

fn latte() -> String {
    fs::read_to_string(fixtures().join("catppuccin-latte/colors.toml")).expect("latte")
}

#[test]
fn legacy_shades_match_omarchy_mix() {
    let raw = r##"
color0 = "#101010"
color1 = "#ff0000"
color2 = "#00ff00"
color3 = "#ffff00"
color4 = "#0000ff"
color5 = "#ff00ff"
color6 = "#00ffff"
color7 = "#eeeeee"
"##;
    let colors = colors_from_str(raw).expect("derive");
    let background = ldnddev_theme::Rgb::new(0x10, 0x10, 0x10);
    assert_eq!(colors.get("background"), Some(background));
    assert_eq!(
        colors.get("darker_background"),
        Some(mix(background, ldnddev_theme::Rgb::new(0, 0, 0), 0.5))
    );
    assert_eq!(
        colors.get("bright_red").unwrap().to_hex(),
        mix(
            ldnddev_theme::Rgb::new(255, 0, 0),
            ldnddev_theme::Rgb::new(255, 255, 255),
            0.2
        )
        .to_hex()
    );
    assert_eq!(colors.get("accent").unwrap().to_hex(), "#0000FF");
}

#[test]
fn maps_tokyo_night_and_latte() {
    let tokyo = map_palette(&colors_from_str(&tokyo()).unwrap());
    assert_eq!(tokyo.get("body_background").unwrap().to_hex(), "#1A1B26");
    assert_eq!(tokyo.get("base_background").unwrap().to_hex(), "#0E0E14");
    assert_eq!(tokyo.get("text_active_focus").unwrap().to_hex(), "#7AA2F7");
    assert_eq!(tokyo.get("folders").unwrap().to_hex(), "#7AA2F7");
    assert_eq!(tokyo.get("links").unwrap().to_hex(), "#449DAB");

    let latte = map_palette(&colors_from_str(&latte()).unwrap());
    assert_eq!(latte.get("body_background").unwrap().to_hex(), "#EFF1F5");
    assert_eq!(latte.get("base_background").unwrap().to_hex(), "#D7D8DC");
    let body = latte.get("body_background").unwrap();
    let shell = latte.get("base_background").unwrap();
    let text = latte.get("text_primary").unwrap();
    let muted = latte.get("text_secondary").unwrap();
    assert!(contrast_ratio(text, body) >= 4.5);
    assert!(contrast_ratio(text, shell) >= 4.5);
    assert!(contrast_ratio(muted, body) >= 3.0);
    assert!(contrast_ratio(muted, shell) >= 3.0);
}

#[test]
fn stock_themes_meet_text_floors() {
    let root = fixtures();
    let mut count = 0;
    for entry in fs::read_dir(&root).expect("fixtures") {
        let entry = entry.expect("entry");
        let colors_path = entry.path().join("colors.toml");
        if !colors_path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&colors_path).expect("read");
        let palette = map_palette(&colors_from_str(&text).expect("parse"));
        let body = palette.get("body_background").unwrap();
        let shell = palette.get("base_background").unwrap();
        let primary = palette.get("text_primary").unwrap();
        let secondary = palette.get("text_secondary").unwrap();
        assert!(
            contrast_ratio(primary, body) >= 4.5 && contrast_ratio(primary, shell) >= 4.5,
            "{} text",
            entry.file_name().to_string_lossy()
        );
        assert!(
            contrast_ratio(secondary, body) >= 3.0 && contrast_ratio(secondary, shell) >= 3.0,
            "{} muted",
            entry.file_name().to_string_lossy()
        );
        count += 1;
    }
    assert_eq!(count, 22);
}

#[test]
fn yaml_round_trip_keeps_link_and_one_override() {
    let mut palette = map_palette(&colors_from_str(&tokyo()).unwrap());
    palette.omarchy = Some(OmarchyChoice::Follow);
    palette.overrides.insert("text_labels".to_string());
    palette.set("text_labels", ldnddev_theme::Rgb::new(0xff, 0xaf, 0x46));
    let yaml = render_yaml(&palette, COLOR_FIELDS);
    assert!(yaml.contains("omarchy: follow\n"));
    assert!(yaml.contains("text_labels:"));
    assert!(!yaml.contains("base_background:"));
    let loaded = load_from_str(&yaml, ParseMode::Strict, ThemeSource::Global).expect("load");
    assert_eq!(loaded.omarchy, Some(OmarchyChoice::Follow));
    assert!(loaded.overrides.contains("text_labels"));
    assert!(!loaded.overrides.contains("base_background"));
}

#[test]
fn user_colors_override_stock_and_background_dirs_are_skipped() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path();
    fs::create_dir_all(root.join("stock/tokyo-night")).unwrap();
    fs::write(root.join("stock/tokyo-night/colors.toml"), tokyo()).unwrap();
    fs::create_dir_all(root.join("user/tokyo-night")).unwrap();
    fs::write(
        root.join("user/tokyo-night/colors.toml"),
        "accent = \"#112233\"\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("user/wallpapers-only")).unwrap();
    fs::write(root.join("user/wallpapers-only/note.txt"), "no colors").unwrap();
    let paths = OmarchyPaths::from_root(root);
    let themes = discover(&paths);
    assert_eq!(themes.len(), 1);
    assert_eq!(themes[0].slug, "tokyo-night");
    assert_eq!(themes[0].origin, ThemeOrigin::Both);
    let colors = ldnddev_theme::load_colors(&paths, &OmarchyChoice::Theme("tokyo-night".into()))
        .expect("load");
    assert_eq!(colors.get("accent").unwrap().to_hex(), "#112233");
    assert_eq!(colors.get("background").unwrap().to_hex(), "#1A1B26");
}

#[test]
fn missing_current_theme_errors() {
    let dir = tempfile::tempdir().expect("tmp");
    let paths = OmarchyPaths::from_root(dir.path());
    let err = ldnddev_theme::load_colors(&paths, &OmarchyChoice::Follow).unwrap_err();
    assert!(err.to_string().contains("missing"));
}

#[test]
fn apply_omarchy_overlays_saved_keys() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path();
    fs::create_dir_all(root.join("stock/tokyo-night")).unwrap();
    fs::write(root.join("stock/tokyo-night/colors.toml"), tokyo()).unwrap();
    let paths = OmarchyPaths::from_root(root);
    let yaml = "version: 1\nomarchy: tokyo-night\ncolors:\n  text_labels: \"#ABCDEF\"\n";
    let loaded = load_from_str(yaml, ParseMode::Lenient, ThemeSource::Local).unwrap();
    let resolved = apply_omarchy(&loaded, &paths).unwrap();
    assert_eq!(resolved.get("body_background").unwrap().to_hex(), "#1A1B26");
    assert_eq!(resolved.get("text_labels").unwrap().to_hex(), "#ABCDEF");
    assert_eq!(
        resolved.omarchy,
        Some(OmarchyChoice::Theme("tokyo-night".into()))
    );
}

#[test]
fn picker_enter_applies_esc_reverts_and_e_edits() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path();
    fs::create_dir_all(root.join("stock/tokyo-night")).unwrap();
    fs::write(root.join("stock/tokyo-night/colors.toml"), tokyo()).unwrap();
    fs::create_dir_all(root.join("state/theme")).unwrap();
    fs::write(root.join("state/theme.name"), "tokyo-night\n").unwrap();
    fs::write(root.join("state/theme/colors.toml"), tokyo()).unwrap();
    let paths = OmarchyPaths::from_root(root);
    let mut picker = ThemePicker::new(Palette::builtin(), &[EXTRA_MODAL_HEADER], paths);
    assert!(picker.lists_omarchy());
    assert!(matches!(
        picker.rows[0],
        ldnddev_theme::PickerRow::Follow { .. }
    ));
    picker.select(1);
    assert_eq!(picker.handle(EditorKey::Enter), PickerOutcome::Applied);
    assert_eq!(
        picker.palette.omarchy,
        Some(OmarchyChoice::Theme("tokyo-night".into()))
    );
    assert!(picker.palette.overrides.is_empty());
    assert_eq!(
        picker.palette.get("body_background").unwrap().to_hex(),
        "#1A1B26"
    );
    assert_eq!(
        picker.handle(EditorKey::Char('e')),
        PickerOutcome::EditColors
    );
    let closed = picker.handle(EditorKey::Esc);
    assert_eq!(closed, PickerOutcome::Closed { reverted: true });
    assert!(picker.palette.omarchy.is_none());
    assert_eq!(
        picker.palette.get("base_background").unwrap().to_hex(),
        "#0F1114"
    );
}
