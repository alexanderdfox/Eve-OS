// SPDX-License-Identifier: MIT OR Apache-2.0

//! Light / dark RGB palettes for chrome, panels, and body text.

/// RGB triple for `gfx::fill_rect` / `draw_str_rgb`.
#[derive(Clone, Copy)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const fn tuple(self) -> (u8, u8, u8) {
        (self.r, self.g, self.b)
    }
}

/// Colors used by the main shell and settings list (TempleOS-inspired chrome).
#[derive(Clone, Copy)]
pub struct UiPalette {
    pub bg_desktop: Rgb,
    pub chrome_bar: Rgb,
    pub chrome_title: Rgb,
    pub tab_strip: Rgb,
    pub tab_active: Rgb,
    pub tab_inactive: Rgb,
    pub tab_text: Rgb,
    pub url_bar: Rgb,
    pub url_button: Rgb,
    pub url_field: Rgb,
    pub status_bg: Rgb,
    pub panel_bg: Rgb,
    pub panel_border: Rgb,
    pub panel_top_line: Rgb,
    pub section_underline: Rgb,
    pub heading: Rgb,
    pub section_tag: Rgb,
    pub text_primary: Rgb,
    pub text_muted: Rgb,
    pub row_a: Rgb,
    pub row_b: Rgb,
    pub row_sep: Rgb,
    pub focus_row: Rgb,
    pub accent: Rgb,
    pub epilepsy_bg: Rgb,
    pub epilepsy_text: Rgb,
    pub epilepsy_warn: Rgb,
    pub epilepsy_btn_outer: Rgb,
    pub epilepsy_btn_inner: Rgb,
    pub epilepsy_btn_text: Rgb,
    pub epilepsy_hint: Rgb,
    pub bios_page_bg: Rgb,
}

impl UiPalette {
    pub const LIGHT: Self = Self {
        bg_desktop: Rgb::new(0xd0, 0xe4, 0xff),
        chrome_bar: Rgb::new(0x34, 0x62, 0xc8),
        chrome_title: Rgb::new(0xff, 0xf4, 0xd6),
        tab_strip: Rgb::new(0xa2, 0xbc, 0xea),
        tab_active: Rgb::new(0xff, 0xff, 0xff),
        tab_inactive: Rgb::new(0xd6, 0xe6, 0xfa),
        tab_text: Rgb::new(0x22, 0x22, 0x22),
        url_bar: Rgb::new(0xc8, 0xe0, 0xff),
        url_button: Rgb::new(0xff, 0xff, 0xff),
        url_field: Rgb::new(0xff, 0xff, 0xff),
        status_bg: Rgb::new(0x38, 0x6c, 0xc8),
        panel_bg: Rgb::new(0xec, 0xf2, 0xfa),
        panel_border: Rgb::new(0x3b, 0x82, 0xf6),
        panel_top_line: Rgb::new(0xfe, 0xfc, 0xff),
        section_underline: Rgb::new(0x3b, 0x82, 0xf6),
        heading: Rgb::new(0x0f, 0x17, 0x2e),
        section_tag: Rgb::new(0x1e, 0x40, 0xad),
        text_primary: Rgb::new(0x22, 0x22, 0x22),
        text_muted: Rgb::new(0x55, 0x55, 0x66),
        row_a: Rgb::new(0xf8, 0xfa, 0xfc),
        row_b: Rgb::new(0xf1, 0xf5, 0xf9),
        row_sep: Rgb::new(0xe2, 0xe8, 0xf0),
        focus_row: Rgb::new(0xe0, 0xf8, 0xff),
        accent: Rgb::new(0x3b, 0x82, 0xf6),
        epilepsy_bg: Rgb::new(0xf4, 0xf6, 0xfc),
        epilepsy_text: Rgb::new(0x22, 0x22, 0x33),
        epilepsy_warn: Rgb::new(0x44, 0x33, 0x33),
        epilepsy_btn_outer: Rgb::new(0xc9, 0x7a, 0x1e),
        epilepsy_btn_inner: Rgb::new(0xe5, 0xa0, 0x38),
        epilepsy_btn_text: Rgb::new(0xff, 0xff, 0xf5),
        epilepsy_hint: Rgb::new(0x77, 0x77, 0x88),
        bios_page_bg: Rgb::new(0xff, 0xff, 0xff),
    };

    pub const DARK: Self = Self {
        bg_desktop: Rgb::new(0x12, 0x16, 0x22),
        chrome_bar: Rgb::new(0x1e, 0x3a, 0x5f),
        chrome_title: Rgb::new(0xff, 0xd7, 0x66),
        tab_strip: Rgb::new(0x25, 0x32, 0x42),
        tab_active: Rgb::new(0x3d, 0x4f, 0x66),
        tab_inactive: Rgb::new(0x2a, 0x35, 0x45),
        tab_text: Rgb::new(0xe8, 0xec, 0xf0),
        url_bar: Rgb::new(0x1f, 0x2d, 0x3d),
        url_button: Rgb::new(0x2d, 0x3b, 0x4d),
        url_field: Rgb::new(0x15, 0x1f, 0x2b),
        status_bg: Rgb::new(0x1a, 0x3a, 0x5c),
        panel_bg: Rgb::new(0x1a, 0x22, 0x30),
        panel_border: Rgb::new(0x3b, 0x82, 0xf6),
        panel_top_line: Rgb::new(0x2a, 0x36, 0x48),
        section_underline: Rgb::new(0x4a, 0x9e, 0xf0),
        heading: Rgb::new(0xe2, 0xe8, 0xf0),
        section_tag: Rgb::new(0x7a, 0xb8, 0xff),
        text_primary: Rgb::new(0xe8, 0xec, 0xf0),
        text_muted: Rgb::new(0x9a, 0xa5, 0xb4),
        row_a: Rgb::new(0x22, 0x2d, 0x3d),
        row_b: Rgb::new(0x28, 0x34, 0x46),
        row_sep: Rgb::new(0x35, 0x42, 0x56),
        focus_row: Rgb::new(0x2a, 0x3f, 0x5a),
        accent: Rgb::new(0x5a, 0x9e, 0xf0),
        epilepsy_bg: Rgb::new(0x15, 0x1c, 0x28),
        epilepsy_text: Rgb::new(0xd0, 0xd6, 0xe0),
        epilepsy_warn: Rgb::new(0xff, 0xb4, 0x7a),
        epilepsy_btn_outer: Rgb::new(0xb8, 0x6a, 0x14),
        epilepsy_btn_inner: Rgb::new(0xd4, 0x88, 0x28),
        epilepsy_btn_text: Rgb::new(0xff, 0xff, 0xf5),
        epilepsy_hint: Rgb::new(0x88, 0x92, 0xa4),
        bios_page_bg: Rgb::new(0x12, 0x16, 0x22),
    };
}
