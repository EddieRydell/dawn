use floem::peniko::Color;

pub const BACKGROUND: &str = "#111111";
pub const PANEL: &str = "#1c1c1c";
pub const PANEL_DARK: &str = "#151515";
pub const SURFACE: &str = "#232323";
pub const STATUS_BAR: &str = "#1a1a1a";
pub const BORDER: &str = "#383838";
pub const TEXT: &str = "#eeeeee";
pub const TEXT_INVERTED: &str = "#ffffff";
pub const MUTED: &str = "#a8a8a8";
pub const SELECTED: &str = "#3f3f3f";
pub const DANGER: &str = "#707070";

pub const APP_FONT: &str = "Segoe UI";
pub const MONO_FONT: &str = "Cascadia Mono";
pub const FONT_UI: f64 = 14.0;
pub const LINE_HEIGHT_UI: f64 = 1.35;
pub const FONT_SMALL: f64 = 14.0;
pub const FONT_EDITOR: f64 = 14.0;

pub const WINDOW_WIDTH: f64 = 1240.0;
pub const WINDOW_HEIGHT: f64 = 800.0;
pub const TITLE_BAR_HEIGHT: f64 = 32.0;
pub const STATUS_BAR_HEIGHT: f64 = 26.0;
pub const MENU_TAB_HEIGHT: f64 = 24.0;
pub const TITLE_BUTTON_WIDTH: f64 = 42.0;
pub const TAB_STRIP_HEIGHT: f64 = 34.0;
pub const TAB_HEIGHT: f64 = 32.0;
pub const TOOLBAR_HEIGHT: f64 = 32.0;
pub const ROW_HEIGHT: f64 = 24.0;
pub const PROJECT_ROW_HEIGHT: f64 = 26.0;
pub const PROJECT_ICON_SIZE: f64 = 15.0;
pub const PROJECT_CARET_SLOT_SIZE: f64 = 18.0;
pub const PROJECT_INDENT_BASE: f64 = 6.0;
pub const PROJECT_INDENT_STEP: f64 = 14.0;
pub const MIN_EDITOR_WIDTH: f64 = 320.0;
pub const GROUPS_PANE_WIDTH: f64 = 240.0;
pub const DEFAULT_LEFT_PANE_WIDTH: f64 = 260.0;
pub const DEFAULT_RIGHT_PANE_WIDTH: f64 = 320.0;
pub const PREVIEW_STEP_SECONDS: f64 = 0.05;
pub const PREVIEW_DURATION_SECONDS: f64 = 30.0;
pub const LAYOUT_NUDGE_STEP: f64 = 0.25;
pub const LAYOUT_DUPLICATE_OFFSET: f64 = 1.0;
pub const FIXTURE_BULB_STEP: f64 = 0.05;
pub const FIXTURE_MIN_BULB_SIZE: f64 = 0.05;

pub const SPACE_2: f64 = 2.0;
pub const SPACE_3: f64 = 3.0;
pub const SPACE_4: f64 = 4.0;
pub const SPACE_5: f64 = 5.0;
pub const SPACE_6: f64 = 6.0;
pub const SPACE_8: f64 = 8.0;
pub const SPACE_9: f64 = 9.0;
pub const SPACE_10: f64 = 10.0;
pub const SPACE_12: f64 = 12.0;
pub const SPACE_24: f64 = 24.0;

pub const BORDER_WIDTH: f64 = 1.0;
pub const SQUARE_RADIUS: f64 = 0.0;

pub fn color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    let value = u32::from_str_radix(hex, 16).expect("theme colors must be valid 6-digit hex");
    Color::rgb8(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}
