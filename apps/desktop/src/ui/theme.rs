use floem::peniko::{Brush, Color};
use floem::style::{CursorStyle, Foreground, Style};
use floem::views::{scroll, ButtonClass, LabelClass, PlaceholderTextClass, TextInputClass};

pub const BACKGROUND: &str = "#111111";
pub const SURFACE: &str = "#232323";
pub const SURFACE_PANEL: &str = "#1c1c1c";
pub const SURFACE_PANEL_DARK: &str = "#151515";
pub const SURFACE_STATUS: &str = "#1a1a1a";
pub const SURFACE_CONTROL: &str = "#2b2b2b";
pub const SURFACE_CONTROL_HOVER: &str = "#363636";
pub const SURFACE_CONTROL_ACTIVE: &str = "#3f3f3f";
pub const SURFACE_CONTROL_DISABLED: &str = "#202020";

pub const TEXT: &str = "#eeeeee";
pub const TEXT_INVERTED: &str = "#ffffff";
pub const TEXT_MUTED: &str = "#a8a8a8";
pub const TEXT_DISABLED: &str = "#707070";

pub const BORDER: &str = "#383838";
pub const BORDER_FOCUS: &str = "#a8a8a8";
pub const BORDER_DISABLED: &str = "#303030";

pub const PANEL: &str = SURFACE_PANEL;
pub const PANEL_DARK: &str = SURFACE_PANEL_DARK;
pub const STATUS_BAR: &str = SURFACE_STATUS;
pub const MUTED: &str = TEXT_MUTED;
pub const SELECTED: &str = SURFACE_CONTROL_ACTIVE;
pub const DANGER: &str = TEXT_DISABLED;

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
pub const CONTROL_RADIUS: f64 = 2.0;
pub const SCROLLBAR_RADIUS: f64 = 2.0;
pub const SCROLLBAR_THICKNESS: f64 = 10.0;

pub fn color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    let value = u32::from_str_radix(hex, 16).expect("theme colors must be valid 6-digit hex");
    Color::rgb8(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

pub fn app_root_style(s: Style) -> Style {
    s.size_full()
        .background(color(BACKGROUND))
        .font_family(APP_FONT.to_string())
        .font_size(FONT_UI)
        .line_height(LINE_HEIGHT_UI as f32)
        .color(color(TEXT))
        .apply(control_class_style())
}

pub fn control_class_style() -> Style {
    let label_style = Style::new()
        .font_family(APP_FONT.to_string())
        .font_size(FONT_UI)
        .line_height(LINE_HEIGHT_UI as f32)
        .color(color(TEXT));

    let button_style = Style::new()
        .height(ROW_HEIGHT)
        .items_center()
        .justify_center()
        .padding_horiz(SPACE_8)
        .padding_vert(SPACE_4)
        .border(BORDER_WIDTH)
        .border_color(color(BORDER))
        .border_radius(CONTROL_RADIUS)
        .background(color(SURFACE_CONTROL))
        .color(color(TEXT))
        .set(Foreground, Brush::Solid(color(TEXT)))
        .hover(|s| s.background(color(SURFACE_CONTROL_HOVER)))
        .active(|s| {
            s.background(color(SURFACE_CONTROL_ACTIVE))
                .color(color(TEXT_INVERTED))
                .set(Foreground, Brush::Solid(color(TEXT_INVERTED)))
        })
        .focus_visible(|s| s.border_color(color(BORDER_FOCUS)))
        .disabled(|s| {
            s.background(color(SURFACE_CONTROL_DISABLED))
                .border_color(color(BORDER_DISABLED))
                .color(color(TEXT_DISABLED))
                .set(Foreground, Brush::Solid(color(TEXT_DISABLED)))
        });

    let input_style = Style::new()
        .height(ROW_HEIGHT)
        .padding_horiz(SPACE_6)
        .padding_vert(SPACE_3)
        .border(BORDER_WIDTH)
        .border_color(color(BORDER))
        .border_radius(CONTROL_RADIUS)
        .background(color(SURFACE_CONTROL))
        .color(color(TEXT))
        .cursor(CursorStyle::Text)
        .hover(|s| s.background(color(SURFACE_CONTROL_HOVER)))
        .focus_visible(|s| s.border_color(color(BORDER_FOCUS)))
        .disabled(|s| {
            s.background(color(SURFACE_CONTROL_DISABLED))
                .border_color(color(BORDER_DISABLED))
                .color(color(TEXT_DISABLED))
        });

    Style::new()
        .class(LabelClass, |_| label_style)
        .class(ButtonClass, |_| button_style)
        .class(TextInputClass, |_| input_style)
        .class(PlaceholderTextClass, |s| {
            s.font_size(FONT_UI).color(color(TEXT_MUTED))
        })
        .apply_custom(
            scroll::ScrollCustomStyle::new()
                .handle_background(color(TEXT_DISABLED))
                .handle_border_radius(SCROLLBAR_RADIUS)
                .handle_thickness(SCROLLBAR_THICKNESS)
                .handle_rounded(false)
                .track_background(color(SURFACE_PANEL_DARK))
                .track_border_radius(SCROLLBAR_RADIUS)
                .track_thickness(SCROLLBAR_THICKNESS)
                .track_rounded(false),
        )
        .class(scroll::Handle, |s| {
            s.hover(|s| s.background(color(TEXT_MUTED)))
                .active(|s| s.background(color(TEXT)))
        })
        .class(scroll::Track, |s| {
            s.hover(|s| s.background(color(SURFACE_PANEL)))
        })
}
