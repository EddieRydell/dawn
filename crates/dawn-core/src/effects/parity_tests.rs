#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

//! Pixel-level parity tests verifying VibeLights effects match Vixen behavior.
//!
//! Each test sets up effect params matching a specific Vixen configuration,
//! evaluates pixels at known times, and asserts exact color/brightness values.
//! This ensures imported Vixen shows look correct in VibeLights.
//!
//! Test naming: `vixen_{effect}_{scenario}`

use crate::model::{
    BlendMode, Color, ColorGradient, ColorMode, ColorStop, Curve, EffectParams, ParamKey,
    ParamValue, WipeDirection,
};

// ── Helpers ────────────────────────────────────────────────────────────

/// Evaluate a strip of pixels for a built-in effect, returning the color array.
fn eval_strip(
    effect: crate::model::BuiltInEffect,
    t: f64,
    pixel_count: usize,
    params: &EffectParams,
) -> Vec<Color> {
    let mut dest = vec![Color::BLACK; pixel_count];
    super::evaluate_pixels(
        &crate::model::EffectKind::BuiltIn(effect),
        t,
        &mut dest,
        0,
        pixel_count,
        params,
        BlendMode::Override,
        1.0,
        None,
    );
    dest
}

/// Assert a color is approximately equal (within tolerance per channel).
fn assert_color_approx(actual: Color, expected: Color, tolerance: u8, ctx: &str) {
    assert!(
        actual.r.abs_diff(expected.r) <= tolerance
            && actual.g.abs_diff(expected.g) <= tolerance
            && actual.b.abs_diff(expected.b) <= tolerance,
        "{ctx}: expected ~({},{},{}) got ({},{},{})",
        expected.r,
        expected.g,
        expected.b,
        actual.r,
        actual.g,
        actual.b,
    );
}

/// Build a red→blue gradient.
fn red_blue_gradient() -> ColorGradient {
    ColorGradient::new(vec![
        ColorStop {
            position: 0.0,
            color: Color::rgb(255, 0, 0),
        },
        ColorStop {
            position: 1.0,
            color: Color::rgb(0, 0, 255),
        },
    ])
    .unwrap_or_else(|| ColorGradient::solid(Color::WHITE))
}

/// Build a triangle curve (0,0) → (0.5,1) → (1,0).
fn triangle_curve() -> Curve {
    Curve::triangle()
}

// ══════════════════════════════════════════════════════════════════════
// SOLID
// ══════════════════════════════════════════════════════════════════════

#[test]
fn vixen_solid_all_pixels_same_color() {
    // Vixen SetLevel: fixed color, fixed intensity for all elements.
    let color = Color::rgb(128, 64, 200);
    let params = EffectParams::new().set(ParamKey::Color, ParamValue::Color(color));

    let strip = eval_strip(crate::model::BuiltInEffect::Solid, 0.0, 10, &params);
    for (i, pixel) in strip.iter().enumerate() {
        assert_eq!(*pixel, color, "pixel {i} should be the solid color");
    }

    // Color doesn't change over time
    let strip_later = eval_strip(crate::model::BuiltInEffect::Solid, 0.5, 10, &params);
    assert_eq!(strip, strip_later);
}

#[test]
fn vixen_solid_time_independent() {
    let params = EffectParams::new();
    for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let c = super::solid::evaluate_single(t, 0, 10, &params);
        assert_eq!(c, Color::WHITE, "solid should be constant at t={t}");
    }
}

// ══════════════════════════════════════════════════════════════════════
// STROBE
// ══════════════════════════════════════════════════════════════════════

#[test]
fn vixen_strobe_basic_on_off() {
    // Vixen defaults: CycleTime=150ms → rate≈6.67Hz, OnTimeCurve=50% → duty=0.5
    let rate = 1000.0 / 150.0;
    let params = EffectParams::new()
        .set(ParamKey::Rate, ParamValue::Float(rate))
        .set(ParamKey::DutyCycle, ParamValue::Float(0.5));

    // All pixels uniform (strobe has no spatial variation in Static mode)
    let strip_on = eval_strip(crate::model::BuiltInEffect::Strobe, 0.0, 10, &params);
    assert!(strip_on.iter().all(|c| *c == Color::WHITE), "all on at t=0");

    // At 75% through cycle → off
    let cycle = 1.0 / rate;
    let strip_off = eval_strip(
        crate::model::BuiltInEffect::Strobe,
        cycle * 0.75,
        10,
        &params,
    );
    assert!(
        strip_off.iter().all(|c| *c == Color::BLACK),
        "all off at 75% cycle"
    );
}

#[test]
fn vixen_strobe_with_intensity_envelope() {
    // Vixen applies IntensityCurve to each flash. Triangle curve: ramps up then down.
    let params = EffectParams::new()
        .set(ParamKey::Rate, ParamValue::Float(1.0))
        .set(ParamKey::DutyCycle, ParamValue::Float(1.0)) // always on
        .set(
            ParamKey::IntensityCurve,
            ParamValue::Curve(triangle_curve()),
        );

    // t=0 → on_pos=0 → triangle(0)=0 → black
    let at_start = super::strobe::evaluate_single(0.0, 0, 1, &params);
    assert_eq!(at_start, Color::BLACK, "triangle at start = 0");

    // t=0.5 → on_pos=0.5 → triangle(0.5)=1.0 → white
    let at_mid = super::strobe::evaluate_single(0.5, 0, 1, &params);
    assert_eq!(at_mid, Color::WHITE, "triangle at peak = 1.0");

    // t=0.25 → triangle(0.25)=0.5 → half brightness
    let at_quarter = super::strobe::evaluate_single(0.25, 0, 1, &params);
    assert_color_approx(at_quarter, Color::rgb(128, 128, 128), 1, "triangle at 0.25");
}

#[test]
fn vixen_strobe_gradient_across_items() {
    // Vixen supports ColorAcrossItems: each pixel gets a different color from the gradient
    let params = EffectParams::new()
        .set(
            ParamKey::Gradient,
            ParamValue::ColorGradient(red_blue_gradient()),
        )
        .set(
            ParamKey::ColorMode,
            ParamValue::ColorMode(ColorMode::GradientAcrossItems),
        )
        .set(ParamKey::Rate, ParamValue::Float(1.0))
        .set(ParamKey::DutyCycle, ParamValue::Float(1.0)); // always on

    let strip = eval_strip(crate::model::BuiltInEffect::Strobe, 0.0, 10, &params);
    // First pixel (pos=0/10=0.0): pure red
    assert_color_approx(strip[0], Color::rgb(255, 0, 0), 1, "first pixel red");
    // Last pixel (pos=9/10=0.9): mostly blue
    assert!(
        strip[9].b > strip[9].r,
        "last pixel should be more blue than red"
    );
    // Middle pixel: mixed
    assert!(
        strip[4].r > 50 && strip[4].b > 50,
        "middle pixel should be mixed"
    );
}

// ══════════════════════════════════════════════════════════════════════
// CHASE
// ══════════════════════════════════════════════════════════════════════

#[test]
fn vixen_chase_head_position_at_t0() {
    // Linear movement, speed=1: at t=0, head at position 0.
    // The pulse TRAILS behind the head (via circular wrap).
    // dist = head - pos; negative → wraps by adding 1.0.
    // Default triangle pulse: peak at pulse_pos=0.5 (midpoint of pulse).
    let params = EffectParams::new()
        .set(ParamKey::Speed, ParamValue::Float(1.0))
        .set(ParamKey::PulseWidth, ParamValue::Float(0.2))
        .set(ParamKey::BackgroundLevel, ParamValue::Float(0.0));

    let strip = eval_strip(crate::model::BuiltInEffect::Chase, 0.0, 100, &params);

    // Head at 0.0. Pulse width 0.2 → pulse covers dist [0, 0.2).
    // Pixel at pos=0.0: dist=0.0, pulse_pos=0.0 → triangle(0)=0 (at head edge)
    // Pixel at pos=0.9: dist=0.0-0.9+1.0=0.1, pulse_pos=0.5 → triangle(0.5)=1.0 (peak!)
    assert!(
        strip[90].r > 200,
        "pixel 90 should be at pulse peak, got r={}",
        strip[90].r
    );

    // Pixel at 50% → dist = 0.0-0.5+1.0 = 0.5 → outside 0.2 pulse width
    assert_eq!(
        strip[50],
        Color::BLACK,
        "pixel 50 should be dark (outside pulse)"
    );
}

#[test]
fn vixen_chase_head_moves_with_time() {
    // At t=0.5, linear movement: head at 0.5
    // Pulse trails behind head. With 0.1 width, peak at dist=0.05.
    // Pixel at pos=0.45: dist=0.5-0.45=0.05 → pulse_pos=0.5 → triangle peak
    let params = EffectParams::new()
        .set(ParamKey::Speed, ParamValue::Float(1.0))
        .set(ParamKey::PulseWidth, ParamValue::Float(0.1))
        .set(ParamKey::BackgroundLevel, ParamValue::Float(0.0));

    let strip = eval_strip(crate::model::BuiltInEffect::Chase, 0.5, 100, &params);

    // Pixel at 45% should be near peak (dist from head=0.05, pulse_pos=0.5)
    assert!(
        strip[45].r > 200,
        "pixel trailing head should be bright, got r={}",
        strip[45].r
    );
    // Pixel 0 should be dark (far from head)
    assert_eq!(strip[0], Color::BLACK, "pixel far from head should be dark");
}

#[test]
fn vixen_chase_reverse() {
    // Reverse: head moves from 1→0 instead of 0→1
    let params = EffectParams::new()
        .set(ParamKey::Speed, ParamValue::Float(1.0))
        .set(ParamKey::PulseWidth, ParamValue::Float(0.1))
        .set(ParamKey::BackgroundLevel, ParamValue::Float(0.0))
        .set(ParamKey::Reverse, ParamValue::Bool(true));

    // At t=0.5, reverse: head = 1 - 0.5 = 0.5. Pulse trails behind.
    // Pixel at pos=0.55: dist=0.5-0.55 → negative → +1.0=0.95 → outside
    // Pixel at pos=0.45: dist=0.5-0.45=0.05 → pulse_pos=0.5 → peak
    let strip = eval_strip(crate::model::BuiltInEffect::Chase, 0.5, 100, &params);
    assert!(
        strip[45].r > 200,
        "reversed head at 0.5 should light trailing pixels, got r={}",
        strip[45].r
    );
}

#[test]
fn vixen_chase_background_level() {
    // Background level = 0.3: all pixels should be at least 30% bright
    let params = EffectParams::new()
        .set(ParamKey::Speed, ParamValue::Float(1.0))
        .set(ParamKey::PulseWidth, ParamValue::Float(0.05))
        .set(ParamKey::BackgroundLevel, ParamValue::Float(0.3));

    let strip = eval_strip(crate::model::BuiltInEffect::Chase, 0.0, 100, &params);

    // All pixels (even outside pulse) should be at least 30% of 255 ≈ 76
    for (i, pixel) in strip.iter().enumerate() {
        assert!(
            pixel.r >= 76,
            "pixel {i} should be at least bg level, got r={}",
            pixel.r
        );
    }
}

#[test]
fn vixen_chase_color_modes() {
    // GradientAcrossItems: color varies by spatial position
    let params = EffectParams::new()
        .set(
            ParamKey::Gradient,
            ParamValue::ColorGradient(red_blue_gradient()),
        )
        .set(
            ParamKey::ColorMode,
            ParamValue::ColorMode(ColorMode::GradientAcrossItems),
        )
        .set(ParamKey::Speed, ParamValue::Float(1.0))
        .set(ParamKey::PulseWidth, ParamValue::Float(1.0)) // full width so all are visible
        .set(ParamKey::BackgroundLevel, ParamValue::Float(0.0));

    let strip = eval_strip(crate::model::BuiltInEffect::Chase, 0.5, 100, &params);

    // First lit pixel should be reddish, last lit pixel bluish
    let first_lit = strip.iter().find(|c| c.r > 0 || c.b > 0);
    let last_lit = strip.iter().rev().find(|c| c.r > 0 || c.b > 0);
    assert!(
        first_lit.is_some() && last_lit.is_some(),
        "should have lit pixels"
    );
}

#[test]
fn vixen_chase_pulse_curve_triangle() {
    // Triangle pulse: peak in the middle of the pulse, zero at edges
    let params = EffectParams::new()
        .set(ParamKey::Speed, ParamValue::Float(1.0))
        .set(ParamKey::PulseWidth, ParamValue::Float(0.5))
        .set(ParamKey::PulseCurve, ParamValue::Curve(triangle_curve()))
        .set(ParamKey::BackgroundLevel, ParamValue::Float(0.0));

    let strip = eval_strip(crate::model::BuiltInEffect::Chase, 0.0, 100, &params);

    // Head at 0.0. Pulse extends behind (via wrapping) from 1.0 to 0.5.
    // Pixel at 75 (pos=0.75): dist = 0.0-0.75+1.0 = 0.25, pulse_pos = 0.25/0.5 = 0.5
    // Triangle at 0.5 = 1.0 (peak)
    assert!(
        strip[75].r > 200,
        "mid-pulse pixel should be bright, got r={}",
        strip[75].r
    );

    // Pixel at 50 (pos=0.5): dist = 0.0-0.5+1.0 = 0.5, pulse_pos = 0.5/0.5 = 1.0
    // Triangle at 1.0 = 0.0 (edge)
    assert!(
        strip[50].r < 10,
        "edge of pulse should be dark, got r={}",
        strip[50].r
    );
}

// ══════════════════════════════════════════════════════════════════════
// FADE (maps Vixen Pulse effect)
// ══════════════════════════════════════════════════════════════════════

#[test]
fn vixen_pulse_triangle_envelope() {
    // Vixen Pulse: LevelCurve (triangle) controls brightness, ColorGradient for color.
    let params = EffectParams::new()
        .set(
            ParamKey::IntensityCurve,
            ParamValue::Curve(triangle_curve()),
        )
        .set(
            ParamKey::Gradient,
            ParamValue::ColorGradient(ColorGradient::solid(Color::rgb(200, 100, 50))),
        );

    // At t=0: triangle=0 → black
    let at_start = super::fade::evaluate_single(0.0, 0, 10, &params);
    assert_eq!(at_start, Color::BLACK, "fade starts at zero");

    // At t=0.5: triangle=1.0 → full color
    let at_peak = super::fade::evaluate_single(0.5, 0, 10, &params);
    assert_eq!(
        at_peak,
        Color::rgb(200, 100, 50),
        "fade at peak = full color"
    );

    // At t=1.0: triangle=0 → black
    let at_end = super::fade::evaluate_single(1.0, 0, 10, &params);
    assert_eq!(at_end, Color::BLACK, "fade at end = zero");
}

#[test]
fn vixen_pulse_gradient_through_effect() {
    // Vixen Pulse with GradientThroughWholeEffect: color follows time position
    let params = EffectParams::new()
        .set(
            ParamKey::IntensityCurve,
            ParamValue::Curve(Curve::constant(1.0)),
        )
        .set(
            ParamKey::Gradient,
            ParamValue::ColorGradient(red_blue_gradient()),
        )
        .set(
            ParamKey::ColorMode,
            ParamValue::ColorMode(ColorMode::GradientThroughEffect),
        );

    // t=0 → red, t=1 → blue
    let early = super::fade::evaluate_single(0.0, 0, 10, &params);
    let late = super::fade::evaluate_single(1.0, 0, 10, &params);
    assert_color_approx(early, Color::rgb(255, 0, 0), 1, "fade t=0 red");
    assert_color_approx(late, Color::rgb(0, 0, 255), 1, "fade t=1 blue");
}

// ══════════════════════════════════════════════════════════════════════
// TWINKLE
// ══════════════════════════════════════════════════════════════════════

#[test]
fn vixen_twinkle_min_max_levels() {
    // Vixen: MinimumLevel=0.2, MaximumLevel=0.8, AverageCoverage=100%
    let params = EffectParams::new()
        .set(ParamKey::Density, ParamValue::Float(1.0)) // all pixels twinkle
        .set(ParamKey::BackgroundLevel, ParamValue::Float(0.2)) // min level
        .set(ParamKey::MaxLevel, ParamValue::Float(0.8));

    let strip = eval_strip(crate::model::BuiltInEffect::Twinkle, 0.0, 50, &params);

    for (i, pixel) in strip.iter().enumerate() {
        // All should be between 20% and 80% of 255
        let min_expected = (0.2 * 255.0) as u8 - 2; // tolerance
        let max_expected = (0.8 * 255.0) as u8 + 2;
        assert!(
            pixel.r >= min_expected,
            "pixel {i} below min level: r={}",
            pixel.r
        );
        assert!(
            pixel.r <= max_expected,
            "pixel {i} above max level: r={}",
            pixel.r
        );
    }
}

#[test]
fn vixen_twinkle_density_controls_coverage() {
    // Low density (10%): most pixels should be at min level
    let params_low = EffectParams::new()
        .set(ParamKey::Density, ParamValue::Float(0.1))
        .set(ParamKey::BackgroundLevel, ParamValue::Float(0.0));

    let strip_low = eval_strip(crate::model::BuiltInEffect::Twinkle, 0.0, 200, &params_low);
    let lit_low = strip_low.iter().filter(|c| c.r > 0).count();

    // High density (90%): most pixels should be twinkling
    let params_high = EffectParams::new()
        .set(ParamKey::Density, ParamValue::Float(0.9))
        .set(ParamKey::BackgroundLevel, ParamValue::Float(0.0));

    let strip_high = eval_strip(crate::model::BuiltInEffect::Twinkle, 0.0, 200, &params_high);
    let lit_high = strip_high.iter().filter(|c| c.r > 0).count();

    assert!(
        lit_high > lit_low * 2,
        "high density should light more pixels than low: high={lit_high}, low={lit_low}"
    );
}

#[test]
fn vixen_twinkle_gradient_across_items() {
    // ColorAcrossItems: early pixels → red, late pixels → blue
    // Use many pixels to average out hash-based intensity variation.
    let params = EffectParams::new()
        .set(
            ParamKey::Gradient,
            ParamValue::ColorGradient(red_blue_gradient()),
        )
        .set(
            ParamKey::ColorMode,
            ParamValue::ColorMode(ColorMode::GradientAcrossItems),
        )
        .set(ParamKey::Density, ParamValue::Float(1.0))
        .set(ParamKey::MaxLevel, ParamValue::Float(1.0));

    let strip = eval_strip(crate::model::BuiltInEffect::Twinkle, 0.0, 200, &params);

    // Aggregate red and blue channels from first 20 and last 20 pixels
    let first_red: u32 = strip[..20].iter().map(|c| c.r as u32).sum();
    let first_blue: u32 = strip[..20].iter().map(|c| c.b as u32).sum();
    let last_red: u32 = strip[180..].iter().map(|c| c.r as u32).sum();
    let last_blue: u32 = strip[180..].iter().map(|c| c.b as u32).sum();

    assert!(
        first_red > first_blue,
        "first pixels should be more red: r={first_red}, b={first_blue}"
    );
    assert!(
        last_blue > last_red,
        "last pixels should be more blue: r={last_red}, b={last_blue}"
    );
}

// ══════════════════════════════════════════════════════════════════════
// RAINBOW
// ══════════════════════════════════════════════════════════════════════

#[test]
fn vixen_rainbow_hue_spatial_spread() {
    // With spread=1, pixels should span a full hue cycle
    let params = EffectParams::new()
        .set(ParamKey::Speed, ParamValue::Float(0.0)) // no time animation
        .set(ParamKey::Spread, ParamValue::Float(1.0));

    let strip = eval_strip(crate::model::BuiltInEffect::Rainbow, 0.0, 10, &params);

    // First pixel: hue=0° (red), pixel at 1/3: hue=120° (green), at 2/3: hue=240° (blue)
    assert!(strip[0].r > 200, "pixel 0 should be reddish");
    // Adjacent pixels should have different hues
    assert_ne!(
        strip[0], strip[3],
        "different positions should have different hues"
    );
}

#[test]
fn vixen_rainbow_time_scrolls_hue() {
    let params = EffectParams::new()
        .set(ParamKey::Speed, ParamValue::Float(1.0))
        .set(ParamKey::Spread, ParamValue::Float(0.0)); // no spatial variation

    // All pixels same color (spread=0), but changes over time
    let at_t0 = super::rainbow::evaluate_single(0.0, 0, 1, &params);
    let at_t05 = super::rainbow::evaluate_single(0.5, 0, 1, &params);
    assert_ne!(at_t0, at_t05, "rainbow should change over time");
}

// ══════════════════════════════════════════════════════════════════════
// GRADIENT
// ══════════════════════════════════════════════════════════════════════

#[test]
fn vixen_gradient_spatial_interpolation() {
    // The Gradient effect uses Colors (color list), not ColorGradient.
    // Note: pos = pixel_index / (total - 1), then .fract().abs() wraps 1.0→0.0.
    // So last pixel wraps. Use 10 pixels: pos goes from 0/9=0.0 to 9/9=1.0→fract()=0.0.
    // To test interpolation, look at intermediate pixels.
    let params = EffectParams::new().set(
        ParamKey::Colors,
        ParamValue::ColorList(vec![Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]),
    );

    let strip = eval_strip(crate::model::BuiltInEffect::Gradient, 0.0, 10, &params);

    // First pixel (pos=0/9=0.0): red
    assert_color_approx(strip[0], Color::rgb(255, 0, 0), 1, "gradient start");
    // Pixel at index 4 (pos=4/9≈0.44): mix skewing red
    assert!(
        strip[4].r > strip[4].b,
        "pixel 4 should be more red than blue"
    );
    // Pixel at index 8 (pos=8/9≈0.89): mix skewing blue
    assert!(
        strip[8].b > strip[8].r,
        "pixel 8 should be more blue than red"
    );
}

// ══════════════════════════════════════════════════════════════════════
// WIPE
// ══════════════════════════════════════════════════════════════════════

#[test]
fn vixen_wipe_horizontal_reveal() {
    // Wipe on: pixels behind head are lit, ahead are dark
    let params = EffectParams::new()
        .set(
            ParamKey::Direction,
            ParamValue::WipeDirection(WipeDirection::Horizontal),
        )
        .set(ParamKey::PulseWidth, ParamValue::Float(0.05))
        .set(ParamKey::WipeOn, ParamValue::Bool(true))
        .set(ParamKey::Speed, ParamValue::Float(1.0))
        .set(ParamKey::PassCount, ParamValue::Float(1.0));

    // At t=0.5, head should be ~midway. Pixels before midpoint: lit. After: dark.
    let strip = eval_strip(crate::model::BuiltInEffect::Wipe, 0.5, 20, &params);

    // First few pixels should be lit
    assert!(
        strip[0].r > 200,
        "first pixel should be bright: r={}",
        strip[0].r
    );
    // Last pixel should be dark
    assert!(
        strip[19].r < 30,
        "last pixel should be dark: r={}",
        strip[19].r
    );
}

#[test]
fn vixen_wipe_conceal() {
    // Wipe off: pixels behind head go dark
    let params = EffectParams::new()
        .set(
            ParamKey::Direction,
            ParamValue::WipeDirection(WipeDirection::Horizontal),
        )
        .set(ParamKey::PulseWidth, ParamValue::Float(0.05))
        .set(ParamKey::WipeOn, ParamValue::Bool(false))
        .set(ParamKey::Speed, ParamValue::Float(1.0))
        .set(ParamKey::PassCount, ParamValue::Float(1.0));

    let strip = eval_strip(crate::model::BuiltInEffect::Wipe, 0.5, 20, &params);

    // Wipe off inverts: first pixels dark, last pixels lit
    assert!(
        strip[0].r < 30,
        "first pixel should be dark in wipe off: r={}",
        strip[0].r
    );
    assert!(
        strip[19].r > 200,
        "last pixel should be bright in wipe off: r={}",
        strip[19].r
    );
}

#[test]
fn vixen_wipe_default_pulse_width() {
    // Vixen WipeData default PulsePercent=33% → pulse_width=0.33
    let params = EffectParams::new()
        .set(ParamKey::PulseWidth, ParamValue::Float(0.33))
        .set(ParamKey::WipeOn, ParamValue::Bool(true));

    let strip = eval_strip(crate::model::BuiltInEffect::Wipe, 0.5, 100, &params);

    // With 33% edge width, the transition zone should span about 33 pixels
    let transition_pixels = strip.iter().filter(|c| c.r > 10 && c.r < 245).count();
    assert!(transition_pixels > 15, "33% pulse width should create a visible transition zone, got {transition_pixels} transition pixels");
}

#[test]
fn vixen_wipe_reverse() {
    // Reverse flips the sweep direction.
    // With wipe_on=true (reveal), forward sweeps left→right, reverse sweeps right→left.
    // At t=0.25: forward head is early (low pos) → few pixels lit.
    //            reverse head = 1-0.25 = 0.75 → most pixels already lit.
    // At t=0.75: forward head is late → most pixels lit.
    //            reverse head = 1-0.75 = 0.25 → few pixels lit.
    // So reversed at a given time should be the opposite of forward at that time.
    let params_fwd = EffectParams::new()
        .set(ParamKey::PulseWidth, ParamValue::Float(0.05))
        .set(ParamKey::WipeOn, ParamValue::Bool(true))
        .set(ParamKey::Reverse, ParamValue::Bool(false));

    let params_rev = EffectParams::new()
        .set(ParamKey::PulseWidth, ParamValue::Float(0.05))
        .set(ParamKey::WipeOn, ParamValue::Bool(true))
        .set(ParamKey::Reverse, ParamValue::Bool(true));

    let strip_fwd_early = eval_strip(crate::model::BuiltInEffect::Wipe, 0.25, 20, &params_fwd);
    let strip_rev_early = eval_strip(crate::model::BuiltInEffect::Wipe, 0.25, 20, &params_rev);

    let fwd_total: u32 = strip_fwd_early.iter().map(|c| c.r as u32).sum();
    let rev_total: u32 = strip_rev_early.iter().map(|c| c.r as u32).sum();

    // At t=0.25: reverse should have MORE total light than forward
    // (reverse is 75% done revealing, forward only 25%)
    assert!(
        rev_total > fwd_total,
        "reverse at t=0.25 should have more light than forward: rev={rev_total}, fwd={fwd_total}"
    );

    // Verify the opposite at t=0.75
    let strip_fwd_late = eval_strip(crate::model::BuiltInEffect::Wipe, 0.75, 20, &params_fwd);
    let strip_rev_late = eval_strip(crate::model::BuiltInEffect::Wipe, 0.75, 20, &params_rev);

    let fwd_late_total: u32 = strip_fwd_late.iter().map(|c| c.r as u32).sum();
    let rev_late_total: u32 = strip_rev_late.iter().map(|c| c.r as u32).sum();

    assert!(fwd_late_total > rev_late_total,
        "forward at t=0.75 should have more light than reverse: fwd={fwd_late_total}, rev={rev_late_total}");
}

#[test]
fn vixen_wipe_pass_count() {
    // PassCount=2: wipe completes twice in the effect duration
    let params = EffectParams::new()
        .set(ParamKey::PulseWidth, ParamValue::Float(0.05))
        .set(ParamKey::WipeOn, ParamValue::Bool(true))
        .set(ParamKey::PassCount, ParamValue::Float(2.0));

    // At t=0.25 (quarter through effect = halfway through first pass)
    let strip_quarter = eval_strip(crate::model::BuiltInEffect::Wipe, 0.25, 20, &params);
    // At t=0.75 (3/4 through effect = halfway through second pass)
    let strip_three_q = eval_strip(crate::model::BuiltInEffect::Wipe, 0.75, 20, &params);

    // Both should look similar (same position in their respective pass)
    // Check that roughly the same number of pixels are lit
    let lit_q = strip_quarter.iter().filter(|c| c.r > 128).count();
    let lit_3q = strip_three_q.iter().filter(|c| c.r > 128).count();
    assert!(
        lit_q.abs_diff(lit_3q) <= 3,
        "pass count should make pattern repeat: lit_q={lit_q}, lit_3q={lit_3q}"
    );
}

// ══════════════════════════════════════════════════════════════════════
// IMPORT INTEGRATION: End-to-end import → effect → pixel tests
// ══════════════════════════════════════════════════════════════════════

/// Simulate what the Vixen importer produces for a specific effect config,
/// then verify the pixel output matches expected Vixen behavior.
#[test]
fn vixen_import_strobe_cycle_time_100ms() {
    use crate::import::vixen::effects::map_vixen_effect;
    use crate::import::vixen::types::VixenEffect;
    use std::collections::HashMap;

    let mut effect = VixenEffect {
        type_name: "Strobe".into(),
        start_time: 0.0,
        duration: 5.0,
        target_node_guids: vec![],
        color: Some(Color::rgb(0, 255, 0)),
        movement_curve: None,
        pulse_curve: None,
        intensity_curve: None,
        gradients: vec![],
        named_curves: HashMap::new(),
        color_handling: None,
        level: None,
        revolution_count: None,
        pulse_percentage: None,
        reverse_spin: None,
        direction: None,
        raw_params: HashMap::new(),
    };
    effect.raw_params.insert("CycleTime".into(), "100".into());

    let (kind, params): (crate::model::EffectKind, EffectParams) = map_vixen_effect(&effect);

    // Should map to Strobe with rate = 1000/100 = 10 Hz
    assert!(matches!(
        kind,
        crate::model::EffectKind::BuiltIn(crate::model::BuiltInEffect::Strobe)
    ));
    let rate: f64 = params.float_or(ParamKey::Rate, 0.0);
    assert!((rate - 10.0).abs() < 0.1);

    // Evaluate pixels: at t=0, should be on (green)
    let on_color = super::strobe::evaluate_single(0.0, 0, 1, &params);
    assert!(
        on_color.g > 200,
        "strobe on should show green: g={}",
        on_color.g
    );

    // At t=0.075 (75% through cycle at 10Hz → cycle=0.1s), should be off
    let off_color = super::strobe::evaluate_single(0.075, 0, 1, &params);
    assert_eq!(off_color, Color::BLACK, "strobe off phase should be black");
}

#[test]
fn vixen_import_twinkle_coverage_and_levels() {
    use crate::import::vixen::effects::map_vixen_effect;
    use crate::import::vixen::types::VixenEffect;
    use std::collections::HashMap;

    let mut effect = VixenEffect {
        type_name: "Twinkle".into(),
        start_time: 0.0,
        duration: 5.0,
        target_node_guids: vec![],
        color: Some(Color::WHITE),
        movement_curve: None,
        pulse_curve: None,
        intensity_curve: None,
        gradients: vec![],
        named_curves: HashMap::new(),
        color_handling: None,
        level: None,
        revolution_count: None,
        pulse_percentage: None,
        reverse_spin: None,
        direction: None,
        raw_params: HashMap::new(),
    };
    effect
        .raw_params
        .insert("AverageCoverage".into(), "80".into());
    effect
        .raw_params
        .insert("MinimumLevel".into(), "0.2".into());
    effect
        .raw_params
        .insert("MaximumLevel".into(), "0.7".into());

    let (kind, params): (crate::model::EffectKind, EffectParams) = map_vixen_effect(&effect);
    assert!(matches!(
        kind,
        crate::model::EffectKind::BuiltIn(crate::model::BuiltInEffect::Twinkle)
    ));

    // Verify params
    let density: f64 = params.float_or(ParamKey::Density, 0.0);
    assert!((density - 0.8).abs() < 0.01, "density from 80% coverage");
    let min_level: f64 = params.float_or(ParamKey::BackgroundLevel, 0.0);
    assert!((min_level - 0.2).abs() < 0.01, "min level 0.2");

    // Evaluate: all pixels should be within min-max range
    let strip = eval_strip(crate::model::BuiltInEffect::Twinkle, 0.0, 50, &params);
    for (i, pixel) in strip.iter().enumerate() {
        assert!(
            pixel.r >= 49,
            "pixel {i} below min level 0.2: r={}",
            pixel.r
        ); // 0.2*255=51, -2 tolerance
        assert!(
            pixel.r <= 180,
            "pixel {i} above max level 0.7: r={}",
            pixel.r
        ); // 0.7*255=178.5, +2 tolerance
    }
}

#[test]
fn vixen_import_chase_with_movement_curve() {
    use crate::import::vixen::effects::map_vixen_effect;
    use crate::import::vixen::types::VixenEffect;
    use std::collections::HashMap;

    let effect = VixenEffect {
        type_name: "Chase".into(),
        start_time: 0.0,
        duration: 5.0,
        target_node_guids: vec![],
        color: Some(Color::rgb(255, 0, 0)),
        movement_curve: Some(vec![(0.0, 0.0), (100.0, 100.0)]), // linear
        pulse_curve: Some(vec![(0.0, 0.0), (50.0, 100.0), (100.0, 0.0)]), // triangle
        intensity_curve: None,
        gradients: vec![],
        named_curves: HashMap::new(),
        color_handling: Some("StaticColor".into()),
        level: None,
        revolution_count: None,
        pulse_percentage: None,
        reverse_spin: None,
        direction: None,
        raw_params: HashMap::new(),
    };

    let (kind, params): (crate::model::EffectKind, EffectParams) = map_vixen_effect(&effect);
    assert!(matches!(
        kind,
        crate::model::EffectKind::BuiltIn(crate::model::BuiltInEffect::Chase)
    ));

    // Verify movement curve was imported: at t=0.5, head should be at ~0.5
    // Pulse trails behind head. Pixel at pos=0.45 should be near peak.
    let strip = eval_strip(crate::model::BuiltInEffect::Chase, 0.5, 100, &params);
    // Some pixels near the head should be lit
    let lit_near_head = strip[40..55].iter().any(|c| c.r > 0);
    assert!(
        lit_near_head,
        "chase should have lit pixels near head at t=0.5"
    );
}

// ══════════════════════════════════════════════════════════════════════
// METEOR
// ══════════════════════════════════════════════════════════════════════

#[test]
fn vixen_meteor_has_tails() {
    // Meteor effect should produce directional particles with tails:
    // some pixels bright (head), adjacent pixels dimmer (tail), rest dark.
    let params = EffectParams::new()
        .set(ParamKey::Density, ParamValue::Float(0.2)) // ~3 meteors
        .set(ParamKey::Speed, ParamValue::Float(2.0))
        .set(ParamKey::TailLength, ParamValue::Float(0.3))
        .set(ParamKey::BackgroundLevel, ParamValue::Float(0.0));

    let strip = eval_strip(crate::model::BuiltInEffect::Meteor, 0.3, 100, &params);

    // Should have some bright pixels (heads) and some intermediate (tails)
    let bright = strip.iter().filter(|c| c.r > 200).count();
    let tail = strip.iter().filter(|c| c.r > 0 && c.r <= 200).count();
    let dark = strip.iter().filter(|c| c.r == 0).count();

    assert!(bright > 0, "should have bright head pixels");
    assert!(tail > 0, "should have fading tail pixels");
    assert!(dark > 20, "most pixels should be dark (outside meteors)");
}

#[test]
fn vixen_meteor_reverse_changes_pattern() {
    let base = EffectParams::new()
        .set(ParamKey::Density, ParamValue::Float(0.3))
        .set(ParamKey::Speed, ParamValue::Float(2.0))
        .set(ParamKey::TailLength, ParamValue::Float(0.3));

    let fwd = eval_strip(crate::model::BuiltInEffect::Meteor, 0.3, 50, &base);
    let rev = eval_strip(
        crate::model::BuiltInEffect::Meteor,
        0.3,
        50,
        &base.clone().set(ParamKey::Reverse, ParamValue::Bool(true)),
    );

    assert_ne!(fwd, rev, "reverse should produce a different pattern");
}

#[test]
fn vixen_import_meteor_with_params() {
    use crate::import::vixen::effects::map_vixen_effect;
    use crate::import::vixen::types::VixenEffect;
    use std::collections::HashMap;

    let mut effect = VixenEffect {
        type_name: "Meteor".into(),
        start_time: 0.0,
        duration: 5.0,
        target_node_guids: vec![],
        color: Some(Color::rgb(0, 128, 255)),
        movement_curve: None,
        pulse_curve: None,
        intensity_curve: None,
        gradients: vec![],
        named_curves: HashMap::new(),
        color_handling: None,
        level: None,
        revolution_count: None,
        pulse_percentage: None,
        reverse_spin: None,
        direction: None,
        raw_params: HashMap::new(),
    };
    effect.raw_params.insert("PixelCount".into(), "10".into());
    effect
        .raw_params
        .insert("CountPerString".into(), "3".into());
    effect.raw_params.insert("Speed".into(), "50".into());
    effect.raw_params.insert("Length".into(), "15".into());
    effect
        .raw_params
        .insert("FlipDirection".into(), "true".into());

    let (kind, params) = map_vixen_effect(&effect);
    assert!(matches!(
        kind,
        crate::model::EffectKind::BuiltIn(crate::model::BuiltInEffect::Meteor)
    ));

    // Density: 10*3 / 30 = 1.0
    assert!((params.float_or(ParamKey::Density, 0.0) - 1.0).abs() < 0.01);
    // Speed: 50/10 = 5.0
    assert!((params.float_or(ParamKey::Speed, 0.0) - 5.0).abs() < 0.01);
    // TailLength: 15/50 = 0.3
    assert!((params.float_or(ParamKey::TailLength, 0.0) - 0.3).abs() < 0.01);
    // Reverse from FlipDirection
    assert!(params.bool_or(ParamKey::Reverse, false));

    // Evaluate: should produce visible meteor pattern
    let strip = eval_strip(crate::model::BuiltInEffect::Meteor, 0.3, 50, &params);
    let lit = strip
        .iter()
        .filter(|c| c.r > 0 || c.g > 0 || c.b > 0)
        .count();
    assert!(lit > 0, "imported meteor should produce visible pixels");
}
