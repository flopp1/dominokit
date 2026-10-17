//! Look and feel: one accent colour, roomier spacing, rounded cards.

use eframe::egui::{self, Color32, Rounding, Stroke};

pub const ACCENT: Color32 = Color32::from_rgb(0x4C, 0x9A, 0xFF);
pub const ACCENT_TEXT: Color32 = Color32::WHITE;
pub const WARN: Color32 = Color32::from_rgb(0xF0, 0xB4, 0x4C);
pub const ERROR: Color32 = Color32::from_rgb(0xF2, 0x6D, 0x6D);
pub const OK: Color32 = Color32::from_rgb(0x6C, 0xC6, 0x7E);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 7.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = 24.0;
    let r = Rounding::same(6.0);
    let v = &mut style.visuals;
    v.window_rounding = Rounding::same(8.0);
    v.widgets.noninteractive.rounding = r;
    v.widgets.inactive.rounding = r;
    v.widgets.hovered.rounding = r;
    v.widgets.active.rounding = r;
    v.widgets.open.rounding = r;
    v.selection.bg_fill = ACCENT.linear_multiply(0.55);
    v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    v.hyperlink_color = ACCENT;
    ctx.set_style(style);
}

/// Colour for track `i` in the overview and track list.
pub fn track_color(track: usize) -> Color32 {
    const TRACK_COLORS: [Color32; 8] = [
        Color32::from_rgb(0x5C, 0xB8, 0x6B),
        Color32::from_rgb(0x5B, 0x8F, 0xE0),
        Color32::from_rgb(0xE0, 0x8C, 0x5B),
        Color32::from_rgb(0xA4, 0x6B, 0xE0),
        Color32::from_rgb(0xD9, 0xC2, 0x5B),
        Color32::from_rgb(0x5B, 0xC8, 0xD1),
        Color32::from_rgb(0xE0, 0x5B, 0x8C),
        Color32::from_rgb(0x96, 0xA3, 0xB0),
    ];
    TRACK_COLORS[track % TRACK_COLORS.len()]
}
