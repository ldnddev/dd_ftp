use crate::rgb::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorField {
    pub key: &'static str,
    pub group: &'static str,
}

pub const COLOR_FIELDS: &[ColorField] = &[
    ColorField {
        key: "base_background",
        group: "Surfaces",
    },
    ColorField {
        key: "body_background",
        group: "Surfaces",
    },
    ColorField {
        key: "modal_background",
        group: "Surfaces",
    },
    ColorField {
        key: "selected_background",
        group: "Surfaces",
    },
    ColorField {
        key: "text_primary",
        group: "Text",
    },
    ColorField {
        key: "text_secondary",
        group: "Text",
    },
    ColorField {
        key: "text_labels",
        group: "Text",
    },
    ColorField {
        key: "text_active_focus",
        group: "Text",
    },
    ColorField {
        key: "modal_labels",
        group: "Text",
    },
    ColorField {
        key: "modal_text",
        group: "Text",
    },
    ColorField {
        key: "border_default",
        group: "Chrome",
    },
    ColorField {
        key: "border_active",
        group: "Chrome",
    },
    ColorField {
        key: "scrollbar",
        group: "Chrome",
    },
    ColorField {
        key: "scrollbar_hover",
        group: "Chrome",
    },
    ColorField {
        key: "input_border_default",
        group: "Inputs",
    },
    ColorField {
        key: "input_border_focus",
        group: "Inputs",
    },
    ColorField {
        key: "input_text_default",
        group: "Inputs",
    },
    ColorField {
        key: "input_text_focus",
        group: "Inputs",
    },
    ColorField {
        key: "cursor",
        group: "Inputs",
    },
    ColorField {
        key: "success",
        group: "Status",
    },
    ColorField {
        key: "warning",
        group: "Status",
    },
    ColorField {
        key: "error",
        group: "Status",
    },
    ColorField {
        key: "info",
        group: "Status",
    },
    ColorField {
        key: "folders",
        group: "Tree",
    },
    ColorField {
        key: "files",
        group: "Tree",
    },
    ColorField {
        key: "links",
        group: "Tree",
    },
];

pub const EXTRA_MODAL_HEADER: ColorField = ColorField {
    key: "modal_header",
    group: "Text",
};
pub const EXTRA_TEXT_DISABLED: ColorField = ColorField {
    key: "text_disabled",
    group: "Text",
};
pub const EXTRA_TEXT_INVERSE: ColorField = ColorField {
    key: "text_inverse",
    group: "Text",
};
pub const EXTRA_SELECTION: ColorField = ColorField {
    key: "selection",
    group: "Chrome",
};

pub const DEFAULT_EXTRA_MODAL_HEADER: Rgb = Rgb::new(0x64, 0xb4, 0xf5);
pub const DEFAULT_EXTRA_TEXT_DISABLED: Rgb = Rgb::new(0xa0, 0xa4, 0xa8);
pub const DEFAULT_EXTRA_TEXT_INVERSE: Rgb = Rgb::new(0xf9, 0xfa, 0xfb);
pub const DEFAULT_EXTRA_SELECTION: Rgb = Rgb::new(0x20, 0x60, 0xa0);

pub fn is_canonical_key(key: &str) -> bool {
    COLOR_FIELDS.iter().any(|f| f.key == key)
}

pub const CANONICAL_DEFAULTS: &[(&str, Rgb)] = &[
    ("base_background", Rgb::new(0x0f, 0x11, 0x14)),
    ("body_background", Rgb::new(0x2a, 0x2d, 0x31)),
    ("modal_background", Rgb::new(0x1c, 0x1e, 0x21)),
    ("selected_background", Rgb::new(0x0f, 0x11, 0x14)),
    ("text_primary", Rgb::new(0xf5, 0xf6, 0xf7)),
    ("text_secondary", Rgb::new(0x9e, 0xa3, 0xaa)),
    ("text_labels", Rgb::new(0xff, 0xaf, 0x46)),
    ("text_active_focus", Rgb::new(0x64, 0xb4, 0xf5)),
    ("modal_labels", Rgb::new(0x64, 0xb4, 0xf5)),
    ("modal_text", Rgb::new(0xf5, 0xf6, 0xf7)),
    ("border_default", Rgb::new(0xf5, 0xf6, 0xf7)),
    ("border_active", Rgb::new(0x64, 0xb4, 0xf5)),
    ("scrollbar", Rgb::new(0xff, 0xa0, 0x87)),
    ("scrollbar_hover", Rgb::new(0x64, 0xb4, 0xf5)),
    ("input_border_default", Rgb::new(0xf5, 0xf6, 0xf7)),
    ("input_border_focus", Rgb::new(0x64, 0xb4, 0xf5)),
    ("input_text_default", Rgb::new(0xf5, 0xf6, 0xf7)),
    ("input_text_focus", Rgb::new(0x64, 0xb4, 0xf5)),
    ("cursor", Rgb::new(0x64, 0xb4, 0xf5)),
    ("success", Rgb::new(0x82, 0xe0, 0xaa)),
    ("warning", Rgb::new(0xf5, 0xc4, 0x69)),
    ("error", Rgb::new(0xe5, 0x73, 0x73)),
    ("info", Rgb::new(0x5d, 0xad, 0xe2)),
    ("folders", Rgb::new(0x64, 0xb4, 0xf5)),
    ("files", Rgb::new(0xff, 0xaf, 0x46)),
    ("links", Rgb::new(0xff, 0xa0, 0x87)),
];

pub fn extra_default(key: &str) -> Option<Rgb> {
    match key {
        "modal_header" => Some(DEFAULT_EXTRA_MODAL_HEADER),
        "text_disabled" => Some(DEFAULT_EXTRA_TEXT_DISABLED),
        "text_inverse" => Some(DEFAULT_EXTRA_TEXT_INVERSE),
        "selection" => Some(DEFAULT_EXTRA_SELECTION),
        _ => None,
    }
}
