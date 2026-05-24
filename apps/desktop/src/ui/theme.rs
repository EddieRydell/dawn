use floem::peniko::Color;

pub const BACKGROUND: &str = "#f4f4f2";
pub const PANEL: &str = "#e7e7e4";
pub const PANEL_DARK: &str = "#d8d8d4";
pub const SURFACE: &str = "#fbfbfa";
pub const BORDER: &str = "#c8c8c4";
pub const TEXT: &str = "#1d1d1f";
pub const MUTED: &str = "#666664";
pub const SELECTED: &str = "#d1d1cd";
pub const DANGER: &str = "#9b2d2d";

pub fn color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    let value = u32::from_str_radix(hex, 16).unwrap_or(0);
    Color::rgb8(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}
