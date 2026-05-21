#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use super::*;
use crate::dsl::compiler::compile;
use crate::dsl::lexer::lex;
use crate::dsl::parser::parse;
use crate::dsl::typeck::type_check;

fn run(src: &str) -> Color {
    run_with_ctx(src, 0.5, 0, 10)
}

fn run_with_ctx(src: &str, t: f64, pixel: usize, pixels: usize) -> Color {
    let tokens = lex(src).unwrap();
    let script = parse(tokens).unwrap();
    let typed = type_check(&script).unwrap();
    let compiled = compile(&typed).unwrap();

    let pos = if pixels > 1 { pixel as f64 / (pixels - 1) as f64 } else { 0.0 };
    let ctx = VmContext {
        t,
        pixel,
        pixels,
        pos,
        pos2d: (pos, 0.0),
        abs_t: 0.0,
        param_values: &[],
        gradients: &[],
        curves: &[],
        colors: &[],
        paths: &[],
    };

    execute(&compiled, &ctx)
}

#[test]
fn solid_red() {
    let color = run("rgb(1.0, 0.0, 0.0)");
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 0);
    assert_eq!(color.b, 0);
}

#[test]
fn solid_white() {
    let color = run("rgb(1.0, 1.0, 1.0)");
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 255);
    assert_eq!(color.b, 255);
}

#[test]
fn color_literal() {
    let color = run("#ff8000");
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 128);
    assert_eq!(color.b, 0);
}

#[test]
fn let_binding() {
    let color = run("let v = 0.5; rgb(v, v, v)");
    assert_eq!(color.r, 128);
    assert_eq!(color.g, 128);
    assert_eq!(color.b, 128);
}

#[test]
fn time_variable() {
    // At t=0.5, sin(0.5 * PI) ≈ 1.0
    let color = run("let s = sin(t * PI); rgb(s, s, s)");
    assert!(color.r > 250, "Expected near-white, got r={}", color.r);
}

#[test]
fn pixel_variable() {
    // pixel 0 of 10 → pos = 0.0
    let c0 = run_with_ctx("rgb(pos, 0.0, 0.0)", 0.0, 0, 10);
    assert_eq!(c0.r, 0);

    // pixel 9 of 10 → pos = 1.0
    let c9 = run_with_ctx("rgb(pos, 0.0, 0.0)", 0.0, 9, 10);
    assert_eq!(c9.r, 255);
}

#[test]
fn if_else() {
    let color_true = run_with_ctx("if t > 0.3 { rgb(1.0, 0.0, 0.0) } else { rgb(0.0, 0.0, 1.0) }", 0.5, 0, 10);
    assert_eq!(color_true.r, 255);
    assert_eq!(color_true.b, 0);

    let color_false = run_with_ctx("if t > 0.3 { rgb(1.0, 0.0, 0.0) } else { rgb(0.0, 0.0, 1.0) }", 0.1, 0, 10);
    assert_eq!(color_false.r, 0);
    assert_eq!(color_false.b, 255);
}

#[test]
fn math_operations() {
    // clamp(2.0, 0.0, 1.0) = 1.0
    let color = run("let x = clamp(2.0, 0.0, 1.0); rgb(x, x, x)");
    assert_eq!(color.r, 255);

    // abs(-0.5) = 0.5
    let color2 = run("let x = abs(-0.5); rgb(x, x, x)");
    assert_eq!(color2.r, 128);
}

#[test]
fn hsv_color() {
    // HSV(0, 1, 1) = pure red
    let color = run("hsv(0.0, 1.0, 1.0)");
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 0);
    assert_eq!(color.b, 0);
}

#[test]
fn color_scale() {
    let color = run("rgb(1.0, 1.0, 1.0).scale(0.5)");
    assert_eq!(color.r, 128);
    assert_eq!(color.g, 128);
    assert_eq!(color.b, 128);
}

#[test]
fn hash_deterministic() {
    let c1 = run("let h = hash(1.0, 2.0); rgb(h, h, h)");
    let c2 = run("let h = hash(1.0, 2.0); rgb(h, h, h)");
    assert_eq!(c1.r, c2.r);
    assert_eq!(c1.g, c2.g);
}

#[test]
fn complex_rainbow() {
    // Rainbow effect: hue varies with position
    let c0 = run_with_ctx("hsv(pos * 360.0, 1.0, 1.0)", 0.0, 0, 10);
    let c5 = run_with_ctx("hsv(pos * 360.0, 1.0, 1.0)", 0.0, 5, 10);
    // Different pixels should give different colors
    assert_ne!(c0, c5);
}

#[test]
fn gradient_param() {
    let src = "param palette: gradient = #000000, #ffffff;\npalette(t)";
    let tokens = lex(src).unwrap();
    let script = parse(tokens).unwrap();
    let typed = type_check(&script).unwrap();
    let compiled = compile(&typed).unwrap();

    let gradient = ColorGradient::two_color(Color::BLACK, Color::WHITE);
    let gradients: Vec<Option<&ColorGradient>> = vec![Some(&gradient)];

    let ctx = VmContext {
        t: 0.5,
        pixel: 0,
        pixels: 10,
        pos: 0.0,
        pos2d: (0.0, 0.0),
        param_values: &[0.0], // gradient params don't use this slot
        abs_t: 0.0,
        gradients: &gradients,
        curves: &[],
        colors: &[],
        paths: &[],
    };

    let color = execute(&compiled, &ctx);
    // At t=0.5, gradient should be ~mid-gray
    assert!((color.r as i16 - 127).abs() <= 2, "Expected ~127, got r={}", color.r);
}

#[test]
fn user_function() {
    let color = run("fn half(x: float) -> float { x * 0.5 }\nlet v = half(1.0); rgb(v, v, v)");
    assert_eq!(color.r, 128);
}

#[test]
fn color_param() {
    let src = "param bg: color = #ff0000;\nbg";
    let tokens = lex(src).unwrap();
    let script = parse(tokens).unwrap();
    let typed = type_check(&script).unwrap();
    let compiled = compile(&typed).unwrap();

    let colors = vec![Some(Color::rgb(0, 255, 0))]; // override to green
    let ctx = VmContext {
        t: 0.0,
        pixel: 0,
        pixels: 1,
        pos: 0.0,
        pos2d: (0.0, 0.0),
        param_values: &[0.0],
        abs_t: 0.0,
        gradients: &[],
        curves: &[],
        colors: &colors,
        paths: &[],
    };
    let color = execute(&compiled, &ctx);
    assert_eq!(color.r, 0);
    assert_eq!(color.g, 255);
    assert_eq!(color.b, 0);
}

#[test]
fn enum_param() {
    let src = "enum Mode { Red, Green, Blue }\nparam mode: Mode = Red;\nif mode == Mode.Red { rgb(1.0, 0.0, 0.0) } else { rgb(0.0, 1.0, 0.0) }";
    let tokens = lex(src).unwrap();
    let script = parse(tokens).unwrap();
    let typed = type_check(&script).unwrap();
    let compiled = compile(&typed).unwrap();

    // mode = 0 (Red)
    let ctx = VmContext {
        t: 0.0,
        pixel: 0,
        pixels: 1,
        pos: 0.0,
        pos2d: (0.0, 0.0),
        param_values: &[0.0],
        abs_t: 0.0,
        gradients: &[],
        curves: &[],
        colors: &[],
        paths: &[],
    };
    let color = execute(&compiled, &ctx);
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 0);

    // mode = 1 (Green)
    let ctx2 = VmContext {
        t: 0.0,
        pixel: 0,
        pixels: 1,
        pos: 0.0,
        pos2d: (0.0, 0.0),
        param_values: &[1.0],
        abs_t: 0.0,
        gradients: &[],
        curves: &[],
        colors: &[],
        paths: &[],
    };
    let color2 = execute(&compiled, &ctx2);
    assert_eq!(color2.r, 0);
    assert_eq!(color2.g, 255);
}

// ── Phase 6: Validation tests ────────────────────────────────
// Compare DSL script output with native Rust effects pixel-for-pixel.

#[test]
fn validate_solid_red_matches_native() {
    // DSL solid red using float params for r/g/b
    let src = r"
param r: float(0.0, 1.0) = 1.0;
param g: float(0.0, 1.0) = 0.0;
param b: float(0.0, 1.0) = 0.0;
rgb(r, g, b)
";
    let tokens = lex(src).unwrap();
    let script = parse(tokens).unwrap();
    let typed = type_check(&script).unwrap();
    let compiled = compile(&typed).unwrap();

    // Native solid: Color::rgb(255, 0, 0)
    let native = Color::rgb(255, 0, 0);

    for pixel in 0..10 {
        let pos = if pixel > 0 { pixel as f64 / 9.0 } else { 0.0 };
        let ctx = VmContext {
            t: 0.5,
            pixel,
            pixels: 10,
            pos,
            pos2d: (pos, 0.0),
            param_values: &[1.0, 0.0, 0.0], // r=1.0, g=0.0, b=0.0
            abs_t: 0.0,
            gradients: &[],
            curves: &[],
            colors: &[],
            paths: &[],
        };
        let dsl_color = execute(&compiled, &ctx);
        assert_eq!(dsl_color.r, native.r, "pixel {pixel}: r mismatch");
        assert_eq!(dsl_color.g, native.g, "pixel {pixel}: g mismatch");
        assert_eq!(dsl_color.b, native.b, "pixel {pixel}: b mismatch");
    }
}

#[test]
fn validate_solid_literal_matches_native() {
    // DSL solid using literal color (simpler, no params needed)
    let dsl_src = "rgb(1.0, 0.0, 0.0)";
    let native = Color::rgb(255, 0, 0);

    for pixel in 0..10 {
        let dsl_color = run_with_ctx(dsl_src, 0.5, pixel, 10);
        assert_eq!(dsl_color, native, "pixel {pixel}: color mismatch");
    }
}

#[test]
fn validate_rainbow_matches_native() {
    // Native rainbow: spatial = pixel_index / pixel_count * spread (divides by total, not total-1)
    // hue = ((t * speed + spatial) * 360.0) % 360.0
    //
    // DSL must use `pixel * 1.0 / pixels` (not `pos`, which is pixel/(pixels-1))
    let dsl_src = r"
param speed: float(0.1, 20.0) = 1.0;
param spread: float(0.1, 10.0) = 1.0;
let spatial = pixel * 1.0 / pixels * spread;
let hue = (t * speed + spatial) * 360.0 % 360.0;
hsv(hue, 1.0, 1.0)
";
    let tokens = lex(dsl_src).unwrap();
    let script = parse(tokens).unwrap();
    let typed = type_check(&script).unwrap();
    let compiled = compile(&typed).unwrap();

    let pixel_count = 10usize;
    let test_times = [0.0, 0.25, 0.5, 0.75, 1.0];

    for &t in &test_times {
        for pixel in 0..pixel_count {
            // Native calculation
            let spatial_native = if pixel_count > 1 {
                (pixel as f64) / (pixel_count as f64) * 1.0
            } else {
                0.0
            };
            let hue_native = ((t * 1.0 + spatial_native) * 360.0) % 360.0;
            let native = Color::from_hsv(hue_native, 1.0, 1.0);

            // DSL calculation
            let pos = if pixel_count > 1 { pixel as f64 / (pixel_count - 1) as f64 } else { 0.0 };
            let ctx = VmContext {
                t,
                pixel,
                pixels: pixel_count,
                pos,
                pos2d: (pos, 0.0),
                param_values: &[1.0, 1.0], // speed=1.0, spread=1.0
                abs_t: 0.0,
                gradients: &[],
                curves: &[],
                colors: &[],
                paths: &[],
            };
            let dsl_color = execute(&compiled, &ctx);

            // Allow ±1 tolerance due to floating point → u8 rounding
            assert!(
                (dsl_color.r as i16 - native.r as i16).abs() <= 1
                && (dsl_color.g as i16 - native.g as i16).abs() <= 1
                && (dsl_color.b as i16 - native.b as i16).abs() <= 1,
                "t={t}, pixel={pixel}: DSL=({},{},{}) native=({},{},{})",
                dsl_color.r, dsl_color.g, dsl_color.b,
                native.r, native.g, native.b
            );
        }
    }
}

#[test]
fn validate_strobe_matches_native() {
    // Native strobe: phase = (t * rate).fract(); if phase < duty_cycle { color } else { black }
    // DSL equivalent:
    let dsl_src = r"
param rate: float(1.0, 50.0) = 10.0;
param duty_cycle: float(0.0, 1.0) = 0.5;
let phase = fract(t * rate);
if phase < duty_cycle {
    rgb(1.0, 1.0, 1.0)
} else {
    rgb(0.0, 0.0, 0.0)
}
";
    let tokens = lex(dsl_src).unwrap();
    let script = parse(tokens).unwrap();
    let typed = type_check(&script).unwrap();
    let compiled = compile(&typed).unwrap();

    let rate = 10.0f64;
    let duty_cycle = 0.5f64;
    let test_times = [0.0, 0.02, 0.05, 0.08, 0.12, 0.25, 0.5, 0.75, 0.99];

    for &t in &test_times {
        // Native
        let phase = (t * rate).fract();
        let native = if phase < duty_cycle { Color::WHITE } else { Color::BLACK };

        // DSL
        let ctx = VmContext {
            t,
            pixel: 0,
            pixels: 1,
            pos: 0.0,
            pos2d: (0.0, 0.0),
            param_values: &[rate, duty_cycle],
            abs_t: 0.0,
            gradients: &[],
            curves: &[],
            colors: &[],
            paths: &[],
        };
        let dsl_color = execute(&compiled, &ctx);

        assert_eq!(
            dsl_color, native,
            "t={t}: DSL=({},{},{}) native=({},{},{})",
            dsl_color.r, dsl_color.g, dsl_color.b,
            native.r, native.g, native.b
        );
    }
}

// ── Issue #72: Bitwise operators ─────────────────────────────

#[test]
fn bitwise_and() {
    // 6 & 3 = 2 → 2/255 ≈ very dark
    let color = run("let x = 6 & 3; let n = x / 8.0; rgb(n, 0.0, 0.0)");
    // 6 & 3 = 2, 2/8 = 0.25 → 64
    assert_eq!(color.r, 64, "6 & 3 = 2, /8.0 → 0.25 → 64, got {}", color.r);
}

#[test]
fn bitwise_or() {
    // 5 | 3 = 7
    let color = run("let x = 5 | 3; let n = x / 8.0; rgb(n, 0.0, 0.0)");
    // 5 | 3 = 7, 7/8 = 0.875 → 223
    assert_eq!(color.r, 223, "5 | 3 = 7, /8.0 → 0.875 → 223, got {}", color.r);
}

#[test]
fn bitwise_xor() {
    // 5 ^ 3 = 6
    let color = run("let x = 5 ^ 3; let n = x / 8.0; rgb(n, 0.0, 0.0)");
    // 5 ^ 3 = 6, 6/8 = 0.75 → 191
    assert_eq!(color.r, 191, "5 ^ 3 = 6, /8.0 → 0.75 → 191, got {}", color.r);
}

#[test]
fn shift_left() {
    // 1 << 3 = 8
    let color = run("let x = 1 << 3; let n = x / 8.0; rgb(n, 0.0, 0.0)");
    assert_eq!(color.r, 255, "1 << 3 = 8, /8.0 → 1.0 → 255, got {}", color.r);
}

#[test]
fn shift_right() {
    // 8 >> 2 = 2
    let color = run("let x = 8 >> 2; let n = x / 8.0; rgb(n, 0.0, 0.0)");
    assert_eq!(color.r, 64, "8 >> 2 = 2, /8.0 → 0.25 → 64, got {}", color.r);
}

#[test]
fn shift_clamped() {
    // Negative shift amounts should be clamped to 0
    let color = run("let x = 8 >> 0; let n = x / 8.0; rgb(n, 0.0, 0.0)");
    assert_eq!(color.r, 255, "8 >> 0 = 8, /8.0 → 1.0 → 255, got {}", color.r);
}

// ── Issue #69: Whitespace-agnostic if/else ──────────────────

#[test]
fn if_else_with_blank_lines() {
    let color = run_with_ctx("if t > 0.3 { rgb(1.0, 0.0, 0.0) } else { rgb(0.0, 0.0, 1.0) }", 0.1, 0, 10);
    assert_eq!(color.r, 0);
    assert_eq!(color.b, 255);
}

// ── Issue #73: Ternary operator ─────────────────────────────

#[test]
fn ternary_true_branch() {
    let color = run("t > 0.3 ? rgb(1.0, 0.0, 0.0) : rgb(0.0, 0.0, 1.0)");
    assert_eq!(color.r, 255);
    assert_eq!(color.b, 0);
}

#[test]
fn ternary_false_branch() {
    let color = run_with_ctx("t > 0.8 ? rgb(1.0, 0.0, 0.0) : rgb(0.0, 0.0, 1.0)", 0.5, 0, 10);
    assert_eq!(color.r, 0);
    assert_eq!(color.b, 255);
}

#[test]
fn ternary_nested() {
    // t=0.5: first condition true
    let color = run("t > 0.3 ? rgb(1.0, 0.0, 0.0) : t > 0.1 ? rgb(0.0, 1.0, 0.0) : rgb(0.0, 0.0, 1.0)");
    assert_eq!(color.r, 255);
}

// ── Issue #72: Power operator ───────────────────────────────

#[test]
fn power_operator() {
    // 2.0 ** 3.0 = 8.0, clamped to 1.0 for color
    let color = run("let x = 2.0 ** 3.0; let n = x / 8.0; rgb(n, 0.0, 0.0)");
    assert_eq!(color.r, 255);
}

#[test]
fn power_right_associative() {
    // 2 ** 3 ** 2 = 2 ** 9 = 512, normalized to check it's 512 not 64
    let color = run("let x = 2.0 ** 3.0 ** 2.0; let n = x / 512.0; rgb(n, 0.0, 0.0)");
    assert_eq!(color.r, 255);
}

// ── Issue #70: Switch/case ──────────────────────────────────

#[test]
fn switch_enum_first_case() {
    let src = "enum Mode { Red, Green, Blue }\nparam mode: Mode = Red;\nswitch mode {\ncase Mode.Red => rgb(1.0, 0.0, 0.0)\ncase Mode.Green => rgb(0.0, 1.0, 0.0)\ndefault => rgb(0.0, 0.0, 1.0)\n}";
    let tokens = lex(src).unwrap();
    let script = parse(tokens).unwrap();
    let typed = type_check(&script).unwrap();
    let compiled = compile(&typed).unwrap();

    let ctx = VmContext {
        t: 0.0, pixel: 0, pixels: 1, pos: 0.0, pos2d: (0.0, 0.0),
        param_values: &[0.0], // Red = 0
        abs_t: 0.0, gradients: &[], curves: &[], colors: &[], paths: &[],
    };
    let color = execute(&compiled, &ctx);
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 0);
}

#[test]
fn switch_enum_second_case() {
    let src = "enum Mode { Red, Green, Blue }\nparam mode: Mode = Red;\nswitch mode {\ncase Mode.Red => rgb(1.0, 0.0, 0.0)\ncase Mode.Green => rgb(0.0, 1.0, 0.0)\ndefault => rgb(0.0, 0.0, 1.0)\n}";
    let tokens = lex(src).unwrap();
    let script = parse(tokens).unwrap();
    let typed = type_check(&script).unwrap();
    let compiled = compile(&typed).unwrap();

    let ctx = VmContext {
        t: 0.0, pixel: 0, pixels: 1, pos: 0.0, pos2d: (0.0, 0.0),
        param_values: &[1.0], // Green = 1
        abs_t: 0.0, gradients: &[], curves: &[], colors: &[], paths: &[],
    };
    let color = execute(&compiled, &ctx);
    assert_eq!(color.r, 0);
    assert_eq!(color.g, 255);
}

#[test]
fn switch_default_fallthrough() {
    let src = "enum Mode { Red, Green, Blue }\nparam mode: Mode = Red;\nswitch mode {\ncase Mode.Red => rgb(1.0, 0.0, 0.0)\ncase Mode.Green => rgb(0.0, 1.0, 0.0)\ndefault => rgb(0.0, 0.0, 1.0)\n}";
    let tokens = lex(src).unwrap();
    let script = parse(tokens).unwrap();
    let typed = type_check(&script).unwrap();
    let compiled = compile(&typed).unwrap();

    let ctx = VmContext {
        t: 0.0, pixel: 0, pixels: 1, pos: 0.0, pos2d: (0.0, 0.0),
        param_values: &[2.0], // Blue = 2 (falls to default)
        abs_t: 0.0, gradients: &[], curves: &[], colors: &[], paths: &[],
    };
    let color = execute(&compiled, &ctx);
    assert_eq!(color.r, 0);
    assert_eq!(color.b, 255);
}

// ── Issue #74: Easing functions ─────────────────────────────

#[test]
fn ease_in_endpoints() {
    // ease_in(0) = 0, ease_in(1) = 1
    let c0 = run_with_ctx("let x = ease_in(t); rgb(x, x, x)", 0.0, 0, 1);
    assert_eq!(c0.r, 0);
    let c1 = run_with_ctx("let x = ease_in(t); rgb(x, x, x)", 1.0, 0, 1);
    assert_eq!(c1.r, 255);
}

#[test]
fn ease_out_endpoints() {
    let c0 = run_with_ctx("let x = ease_out(t); rgb(x, x, x)", 0.0, 0, 1);
    assert_eq!(c0.r, 0);
    let c1 = run_with_ctx("let x = ease_out(t); rgb(x, x, x)", 1.0, 0, 1);
    assert_eq!(c1.r, 255);
}

#[test]
fn ease_in_out_endpoints() {
    let c0 = run_with_ctx("let x = ease_in_out(t); rgb(x, x, x)", 0.0, 0, 1);
    assert_eq!(c0.r, 0);
    let c1 = run_with_ctx("let x = ease_in_out(t); rgb(x, x, x)", 1.0, 0, 1);
    assert_eq!(c1.r, 255);
}

#[test]
fn ease_in_cubic_midpoint() {
    // ease_in_cubic(0.5) = 0.125
    let c = run_with_ctx("let x = ease_in_cubic(t); rgb(x, x, x)", 0.5, 0, 1);
    assert_eq!(c.r, 32, "ease_in_cubic(0.5) ≈ 0.125 → 32, got {}", c.r);
}

#[test]
fn ease_out_cubic_midpoint() {
    // ease_out_cubic(0.5) = (0.5-1)^3 + 1 = -0.125 + 1 = 0.875
    let c = run_with_ctx("let x = ease_out_cubic(t); rgb(x, x, x)", 0.5, 0, 1);
    assert_eq!(c.r, 223, "ease_out_cubic(0.5) ≈ 0.875 → 223, got {}", c.r);
}

#[test]
fn ease_in_out_cubic_symmetry() {
    // ease_in_out_cubic should be symmetric: f(0.25) + f(0.75) ≈ 1.0
    let c_lo = run_with_ctx("let x = ease_in_out_cubic(t); rgb(x, x, x)", 0.25, 0, 1);
    let c_hi = run_with_ctx("let x = ease_in_out_cubic(t); rgb(x, x, x)", 0.75, 0, 1);
    let sum = c_lo.r as u16 + c_hi.r as u16;
    assert!((sum as i16 - 255).abs() <= 1, "symmetry: {} + {} should ≈ 255", c_lo.r, c_hi.r);
}

// ── Issue #77: Deterministic randomness ─────────────────────

#[test]
fn hash3_deterministic() {
    let c1 = run("let h = hash3(1.0, 2.0, 3.0); rgb(h, h, h)");
    let c2 = run("let h = hash3(1.0, 2.0, 3.0); rgb(h, h, h)");
    assert_eq!(c1.r, c2.r);
    // Different inputs should give different output
    let c3 = run("let h = hash3(1.0, 2.0, 4.0); rgb(h, h, h)");
    assert_ne!(c1.r, c3.r, "Different seed should give different value");
}

#[test]
fn random_in_unit_range() {
    // random returns hash(x, 0) which is in [0, 1]
    let c = run("let r = random(42.0); rgb(r, r, r)");
    assert!(c.r > 0 && c.r < 255, "random should produce value in (0, 1), got {}", c.r);
}

#[test]
fn random_range_within_bounds() {
    // random_range(seed, min, max) should be in [min, max] → pixel [51, 204]
    let c = run("let r = random_range(42.0, 0.2, 0.8); rgb(r, r, r)");
    assert!(c.r >= 51 && c.r <= 204, "random_range(42.0, 0.2, 0.8) should be in [51, 204], got {}", c.r);
}

// ── Issue #78: Noise functions ──────────────────────────────

#[test]
fn noise1_deterministic() {
    let c1 = run("let n = abs(noise(5.5)); rgb(n, n, n)");
    let c2 = run("let n = abs(noise(5.5)); rgb(n, n, n)");
    assert_eq!(c1.r, c2.r);
}

#[test]
fn noise2_varies_with_input() {
    // Use non-integer coordinates to avoid zero crossings
    let c1 = run("let n = abs(noise2(1.3, 2.7)); rgb(n, n, n)");
    let c2 = run("let n = abs(noise2(4.6, 8.1)); rgb(n, n, n)");
    // Different inputs should produce different outputs
    assert_ne!(c1.r, c2.r, "noise2 with different inputs should differ");
}

#[test]
fn noise3_deterministic() {
    let c1 = run("let n = abs(noise3(1.0, 2.0, 3.0)); rgb(n, n, n)");
    let c2 = run("let n = abs(noise3(1.0, 2.0, 3.0)); rgb(n, n, n)");
    assert_eq!(c1.r, c2.r);
}

#[test]
fn fbm_more_detail_than_single_octave() {
    // FBM with 1 octave is just perlin2; more octaves add detail
    let c1 = run("let n = abs(fbm(3.5, 7.2, 1.0)); rgb(n, n, n)");
    let c4 = run("let n = abs(fbm(3.5, 7.2, 4.0)); rgb(n, n, n)");
    // With different octave counts, results should differ
    assert_ne!(c1.r, c4.r, "fbm with 1 vs 4 octaves should differ");
}

#[test]
fn worley2_in_unit_range() {
    // worley2 returns [0, 1], so the color channel should be a valid value
    let c = run("let n = worley2(5.5, 3.2); rgb(n, n, n)");
    // Value should be non-zero (not at a cell center) and less than 1.0
    assert!(c.r > 0, "worley2 should return non-zero for most inputs");
}

#[test]
fn worley2_deterministic() {
    let c1 = run("let n = worley2(5.5, 3.2); rgb(n, n, n)");
    let c2 = run("let n = worley2(5.5, 3.2); rgb(n, n, n)");
    assert_eq!(c1.r, c2.r);
}

#[test]
fn noise_at_integer_boundaries() {
    // Perlin noise at integer coordinates should be 0 (or very close)
    let c = run("let n = noise(0.0); let v = abs(n); rgb(v, v, v)");
    assert!(c.r <= 1, "noise at integer boundary should be ~0, got {}", c.r);
}

// ── Color arithmetic operators ──────────────────────────────

#[test]
fn color_mul_float() {
    // #ff8000 * 0.5 → half-brightness orange
    let color = run("#ff8000 * 0.5");
    assert_eq!(color.r, 128);
    assert_eq!(color.g, 64);
    assert_eq!(color.b, 0);
}

#[test]
fn float_mul_color() {
    // 0.5 * #ff8000 → same result (commutative)
    let color = run("0.5 * #ff8000");
    assert_eq!(color.r, 128);
    assert_eq!(color.g, 64);
    assert_eq!(color.b, 0);
}

#[test]
fn color_add_color() {
    // #ff0000 + #00ff00 → yellow
    let color = run("#ff0000 + #00ff00");
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 255);
    assert_eq!(color.b, 0);
}

#[test]
fn color_sub_color() {
    // #ff0000 - #000100 → saturating subtract
    let color = run("#ff0000 - #000100");
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 0); // max(0, 0 - 1) = 0
    assert_eq!(color.b, 0);
}

#[test]
fn color_add_saturates() {
    // #ff8080 + #ff8080 → saturating add (255, 255, 255 capped)
    let color = run("#ff8080 + #ff8080");
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 255); // 128+128=256 → 255
    assert_eq!(color.b, 255);
}

#[test]
fn float_cast() {
    // float(3) should compile and equal 3.0
    let color = run("let x = float(3); let n = x / 3.0; rgb(n, n, n)");
    assert_eq!(color.r, 255);
}

#[test]
fn gradient_mul_brightness_pattern() {
    // The pattern that kept failing: gradient(pos) * brightness
    // Simulate with a color variable instead of gradient param
    let color = run("let c = #ff8000; let brightness = 0.5; c * brightness");
    assert_eq!(color.r, 128);
    assert_eq!(color.g, 64);
    assert_eq!(color.b, 0);
}

#[test]
fn color_mul_int() {
    // color * int should auto-coerce
    let color = run("#808080 * 1");
    assert_eq!(color.r, 128);
    assert_eq!(color.g, 128);
    assert_eq!(color.b, 128);
}

// ── Vec2 arithmetic operators ───────────────────────────────

#[test]
fn vec2_add() {
    // vec2(0.2, 0.3) + vec2(0.1, 0.2) → (0.3, 0.5) → use as color channels
    let color = run("let v = vec2(0.2, 0.3) + vec2(0.1, 0.2); rgb(v.x, v.y, 0.0)");
    assert!((color.r as i16 - 77).abs() <= 1); // 0.3 * 255 ≈ 77
    assert_eq!(color.g, 128); // 0.5 * 255 = 128 (rounded)
}

#[test]
fn vec2_sub() {
    let color = run("let v = vec2(0.5, 0.8) - vec2(0.2, 0.3); rgb(v.x, v.y, 0.0)");
    assert!((color.r as i16 - 77).abs() <= 1); // 0.3 * 255 ≈ 77
    assert_eq!(color.g, 128); // 0.5 * 255 = 128
}

#[test]
fn vec2_mul_float() {
    let color = run("let v = vec2(0.4, 0.6) * 0.5; rgb(v.x, v.y, 0.0)");
    assert_eq!(color.r, 51); // 0.2 * 255 = 51
    assert!((color.g as i16 - 77).abs() <= 1); // 0.3 * 255 ≈ 77
}

#[test]
fn float_mul_vec2() {
    // Commutative: 0.5 * vec2 should equal vec2 * 0.5
    let color = run("let v = 0.5 * vec2(0.4, 0.6); rgb(v.x, v.y, 0.0)");
    assert_eq!(color.r, 51);
    assert!((color.g as i16 - 77).abs() <= 1);
}

#[test]
fn vec2_spatial_pattern() {
    // Common spatial pattern: offset = pos2d - center, then scale
    let color = run_with_ctx(
        "@spatial true\nlet center = vec2(0.5, 0.5); let offset = pos2d - center; let d = length(offset * 2.0); rgb(d, d, d)",
        0.0, 0, 10,
    );
    // pixel 0, pos2d = (0.0, 0.0), offset = (-0.5, -0.5), *2 = (-1, -1), length ≈ 1.414
    assert!(color.r > 250); // clamped to 1.0 → 255
}

#[test]
fn vec2_mul_int() {
    // vec2 * int should auto-coerce
    let color = run("let v = vec2(0.3, 0.5) * 1; rgb(v.x, v.y, 0.0)");
    assert!((color.r as i16 - 77).abs() <= 1);
    assert_eq!(color.g, 128);
}

// ── Color mix() ─────────────────────────────────────────────

#[test]
fn color_mix_midpoint() {
    // mix(red, blue, 0.5) → purple-ish
    let color = run("mix(#ff0000, #0000ff, 0.5)");
    assert!((color.r as i16 - 128).abs() <= 1, "expected ~128 r, got {}", color.r);
    assert_eq!(color.g, 0);
    assert!((color.b as i16 - 128).abs() <= 1, "expected ~128 b, got {}", color.b);
}

#[test]
fn color_mix_at_zero() {
    // mix(red, blue, 0.0) → red
    let color = run("mix(#ff0000, #0000ff, 0.0)");
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 0);
    assert_eq!(color.b, 0);
}

#[test]
fn color_mix_at_one() {
    // mix(red, blue, 1.0) → blue
    let color = run("mix(#ff0000, #0000ff, 1.0)");
    assert_eq!(color.r, 0);
    assert_eq!(color.g, 0);
    assert_eq!(color.b, 255);
}

#[test]
fn float_mix_still_works() {
    // Ensure standard mix(float, float, float) still works
    let color = run("let x = mix(0.0, 1.0, 0.5); rgb(x, x, x)");
    assert_eq!(color.r, 128);
}

// ── color.lerp() method ─────────────────────────────────────

#[test]
fn color_lerp_midpoint() {
    let color = run("#ff0000.lerp(#0000ff, 0.5)");
    assert!((color.r as i16 - 128).abs() <= 1, "expected ~128 r, got {}", color.r);
    assert_eq!(color.g, 0);
    assert!((color.b as i16 - 128).abs() <= 1, "expected ~128 b, got {}", color.b);
}

#[test]
fn color_lerp_at_zero() {
    let color = run("#ff0000.lerp(#00ff00, 0.0)");
    assert_eq!(color.r, 255);
    assert_eq!(color.g, 0);
}

#[test]
fn color_lerp_at_one() {
    let color = run("#ff0000.lerp(#00ff00, 1.0)");
    assert_eq!(color.r, 0);
    assert_eq!(color.g, 255);
}

// ── HSV field accessors ─────────────────────────────────────

#[test]
fn color_hue_red() {
    // Pure red: hue = 0
    let color = run("let h = #ff0000.hue / 360.0; rgb(h, h, h)");
    assert_eq!(color.r, 0, "red hue should be 0, got {}", color.r);
}

#[test]
fn color_hue_green() {
    // Pure green: hue = 120
    let color = run("let h = #00ff00.hue / 360.0; rgb(h, h, h)");
    // 120/360 = 0.333 → 85
    assert!((color.r as i16 - 85).abs() <= 1, "green hue/360 ≈ 0.333 → 85, got {}", color.r);
}

#[test]
fn color_saturation_pure() {
    // Pure red: saturation = 1.0
    let color = run("let s = #ff0000.saturation; rgb(s, s, s)");
    assert_eq!(color.r, 255, "pure red saturation should be 1.0, got {}", color.r);
}

#[test]
fn color_saturation_gray() {
    // Gray: saturation = 0.0
    let color = run("let s = #808080.saturation; rgb(s, s, s)");
    assert_eq!(color.r, 0, "gray saturation should be 0.0, got {}", color.r);
}

#[test]
fn color_value_full() {
    // Pure red: value = 1.0
    let color = run("let v = #ff0000.value; rgb(v, v, v)");
    assert_eq!(color.r, 255, "pure red value should be 1.0, got {}", color.r);
}

#[test]
fn color_value_half() {
    // Half-brightness red: value = 0.5
    let color = run("let v = #800000.value; rgb(v, v, v)");
    assert!((color.r as i16 - 128).abs() <= 1, "#800000 value ≈ 0.502 → 128, got {}", color.r);
}

// ── float(bool) coercion ────────────────────────────────────

#[test]
fn float_bool_true() {
    let color = run("let x = float(true); rgb(x, x, x)");
    assert_eq!(color.r, 255, "float(true) should be 1.0");
}

#[test]
fn float_bool_false() {
    let color = run("let x = float(false); rgb(x, x, x)");
    assert_eq!(color.r, 0, "float(false) should be 0.0");
}

#[test]
fn float_bool_expression() {
    // float(t > 0.3) with t=0.5 → true → 1.0
    let color = run("let x = float(t > 0.3); rgb(x, x, x)");
    assert_eq!(color.r, 255, "float(t > 0.3) with t=0.5 should be 1.0");
}

#[test]
fn float_bool_mask_pattern() {
    // Common pattern: use float(bool) as a multiplier mask
    let _color = run("let mask = float(pos > 0.5); rgb(mask, 0.0, 0.0)");
    // pixel 0, pos=0.0 → false → 0.0
    let c0 = run_with_ctx("let mask = float(pos > 0.5); rgb(mask, 0.0, 0.0)", 0.0, 0, 10);
    assert_eq!(c0.r, 0);
    // pixel 9, pos=1.0 → true → 1.0
    let c9 = run_with_ctx("let mask = float(pos > 0.5); rgb(mask, 0.0, 0.0)", 0.0, 9, 10);
    assert_eq!(c9.r, 255);
}

// ── map() function ──────────────────────────────────────────

#[test]
fn map_linear() {
    // map(0.5, 0.0, 1.0, 0.0, 255.0) / 255.0 → 0.5
    let color = run("let x = map(0.5, 0.0, 1.0, 0.0, 1.0); rgb(x, x, x)");
    assert_eq!(color.r, 128, "map identity should preserve value");
}

#[test]
fn map_remap_range() {
    // map(5.0, 0.0, 10.0, 0.0, 1.0) → 0.5
    let color = run("let x = map(5.0, 0.0, 10.0, 0.0, 1.0); rgb(x, x, x)");
    assert_eq!(color.r, 128, "map(5, 0-10, 0-1) should be 0.5 → 128");
}

#[test]
fn map_zero_range() {
    // map(x, 5.0, 5.0, 0.0, 1.0) → out_min when in_min == in_max
    let color = run("let x = map(5.0, 5.0, 5.0, 0.0, 1.0); rgb(x, x, x)");
    assert_eq!(color.r, 0, "map with zero input range should return out_min");
}

#[test]
fn map_inverted() {
    // map(0.0, 0.0, 1.0, 1.0, 0.0) → 1.0 (inverted output)
    let color = run("let x = map(0.0, 0.0, 1.0, 1.0, 0.0); rgb(x, x, x)");
    assert_eq!(color.r, 255, "map(0, 0-1, 1-0) should be 1.0");
}

// ── angle() and from_angle() ────────────────────────────────

#[test]
fn angle_basic() {
    // angle(vec2(1, 0)) = 0 radians
    let color = run("let a = angle(vec2(1.0, 0.0)); let n = abs(a); rgb(n, n, n)");
    assert_eq!(color.r, 0, "angle of (1,0) should be 0 radians");
}

#[test]
fn angle_90_degrees() {
    // angle(vec2(0, 1)) = PI/2 ≈ 1.5708
    let color = run("let a = angle(vec2(0.0, 1.0)); let n = a / PI; rgb(n, n, n)");
    // PI/2 / PI = 0.5 → 128
    assert_eq!(color.r, 128, "angle(0,1)/PI should be 0.5 → 128, got {}", color.r);
}

#[test]
fn from_angle_zero() {
    // from_angle(0) = vec2(1, 0)
    let color = run("let v = from_angle(0.0); rgb(v.x, abs(v.y), 0.0)");
    assert_eq!(color.r, 255, "from_angle(0).x should be 1.0");
    assert_eq!(color.g, 0, "from_angle(0).y should be ~0.0");
}

#[test]
fn from_angle_pi_half() {
    // from_angle(PI/2) = vec2(0, 1)
    let color = run("let v = from_angle(PI / 2.0); rgb(abs(v.x), v.y, 0.0)");
    assert!(color.r <= 1, "from_angle(PI/2).x should be ~0, got {}", color.r);
    assert_eq!(color.g, 255, "from_angle(PI/2).y should be 1.0");
}

#[test]
fn angle_from_angle_roundtrip() {
    // from_angle(angle(v)) should return unit vector in same direction
    let color = run("let v = vec2(3.0, 4.0); let a = angle(v); let u = from_angle(a); let d = length(u); rgb(d, d, d)");
    assert_eq!(color.r, 255, "from_angle(angle(v)) should have length 1.0");
}

// ── rotate() ────────────────────────────────────────────────

#[test]
fn rotate_zero() {
    // rotate(v, 0) = v
    let color = run("let v = rotate(vec2(0.5, 0.3), 0.0); rgb(v.x, v.y, 0.0)");
    assert_eq!(color.r, 128, "rotate by 0 should preserve x");
    assert!((color.g as i16 - 77).abs() <= 1, "rotate by 0 should preserve y");
}

#[test]
fn rotate_90_degrees() {
    // rotate(vec2(1, 0), PI/2) ≈ vec2(0, 1)
    let color = run("let v = rotate(vec2(1.0, 0.0), PI / 2.0); rgb(abs(v.x), v.y, 0.0)");
    assert!(color.r <= 1, "rotate(1,0) by PI/2: x should be ~0, got {}", color.r);
    assert_eq!(color.g, 255, "rotate(1,0) by PI/2: y should be 1.0, got {}", color.g);
}

#[test]
fn rotate_180_degrees() {
    // rotate(vec2(1, 0), PI) ≈ vec2(-1, 0)
    let color = run("let v = rotate(vec2(1.0, 0.0), PI); let x = abs(v.x + 1.0); rgb(x, abs(v.y), 0.0)");
    // v.x ≈ -1, so v.x + 1 ≈ 0, abs ≈ 0
    assert!(color.r <= 1, "rotate by PI: x should be ~-1.0");
    assert!(color.g <= 1, "rotate by PI: y should be ~0.0");
}

// ── VM ↔ ops.rs parity tests ─────────────────────────────────────
//
// clamp, mix, smoothstep, and map are intentionally inlined in the VM
// for performance (see ops.rs header comment). These tests ensure the
// VM's inline implementations stay in sync with the canonical versions
// in ops::eval_builtin_fn.

#[test]
fn vm_ops_parity_clamp() {
    let cases: &[(f64, f64, f64)] = &[
        (0.5, 0.0, 1.0),
        (-1.0, 0.0, 1.0),
        (2.0, 0.0, 1.0),
        (0.3, 0.3, 0.3),
    ];
    for &(x, lo, hi) in cases {
        let vm_result = run(&format!("let v = clamp({x}, {lo}, {hi}); rgb(v, 0.0, 0.0)")).r;
        let ops_result = ops::eval_builtin_fn("clamp", &[x, lo, hi]).unwrap();
        let ops_u8 = (ops_result.clamp(0.0, 1.0) * 255.0).round() as u8;
        assert_eq!(vm_result, ops_u8, "clamp({x}, {lo}, {hi}): VM={vm_result} ops={ops_u8}");
    }
}

#[test]
fn vm_ops_parity_mix() {
    let cases: &[(f64, f64, f64)] = &[
        (0.0, 1.0, 0.0),
        (0.0, 1.0, 0.5),
        (0.0, 1.0, 1.0),
        (0.2, 0.8, 0.25),
    ];
    for &(a, b, t) in cases {
        let vm_result = run(&format!("let v = mix({a}, {b}, {t}); rgb(v, 0.0, 0.0)")).r;
        let ops_result = ops::eval_builtin_fn("mix", &[a, b, t]).unwrap();
        let ops_u8 = (ops_result.clamp(0.0, 1.0) * 255.0).round() as u8;
        assert_eq!(vm_result, ops_u8, "mix({a}, {b}, {t}): VM={vm_result} ops={ops_u8}");
    }
}

#[test]
fn vm_ops_parity_smoothstep() {
    let cases: &[(f64, f64, f64)] = &[
        (0.0, 1.0, 0.5),
        (0.0, 1.0, 0.0),
        (0.0, 1.0, 1.0),
        (0.0, 1.0, -0.5),
        (0.0, 1.0, 1.5),
        (1.0, 0.0, 0.5),  // edge0 >= edge1 → 0.0
    ];
    for &(e0, e1, x) in cases {
        let vm_result = run(&format!("let v = smoothstep({e0}, {e1}, {x}); rgb(v, 0.0, 0.0)")).r;
        let ops_result = ops::eval_builtin_fn("smoothstep", &[e0, e1, x]).unwrap();
        let ops_u8 = (ops_result.clamp(0.0, 1.0) * 255.0).round() as u8;
        assert_eq!(vm_result, ops_u8, "smoothstep({e0}, {e1}, {x}): VM={vm_result} ops={ops_u8}");
    }
}

#[test]
fn vm_ops_parity_map() {
    let cases: &[(f64, f64, f64, f64, f64)] = &[
        (0.5, 0.0, 1.0, 0.0, 1.0),
        (0.0, 0.0, 10.0, 0.0, 1.0),
        (10.0, 0.0, 10.0, 0.0, 1.0),
        (5.0, 0.0, 10.0, 0.0, 1.0),
        (5.0, 5.0, 5.0, 0.0, 1.0),  // zero range → out_min
    ];
    for &(x, in_min, in_max, out_min, out_max) in cases {
        let vm_result = run(&format!(
            "let v = map({x}, {in_min}, {in_max}, {out_min}, {out_max}); rgb(v, 0.0, 0.0)"
        )).r;
        let ops_result = ops::eval_builtin_fn("map", &[x, in_min, in_max, out_min, out_max]).unwrap();
        let ops_u8 = (ops_result.clamp(0.0, 1.0) * 255.0).round() as u8;
        assert_eq!(
            vm_result, ops_u8,
            "map({x}, {in_min}, {in_max}, {out_min}, {out_max}): VM={vm_result} ops={ops_u8}"
        );
    }
}

// ── Optimization semantic equivalence ───────────────────────────
// Compile with and without optimizations, run both, assert identical output.
// This is a single harness that protects every future optimization pass:
// if constant folding or peephole changes semantics, these tests catch it.

/// Compile source WITHOUT optimizations (no constant folding, no peephole).
fn compile_unoptimized(src: &str) -> crate::dsl::compiler::CompiledScript {
    let tokens = lex(src).unwrap();
    let script = parse(tokens).unwrap();
    let typed = type_check(&script).unwrap();
    // Skip fold_constants and peephole — compile directly from typed AST.
    crate::dsl::compiler::compile(&typed).unwrap()
}

/// Compile source WITH full optimizations (the normal pipeline).
fn compile_optimized(src: &str) -> crate::dsl::compiler::CompiledScript {
    crate::dsl::compile_source(src).unwrap()
}

/// Run a compiled script at various (t, pixel, pixels) combinations and
/// collect the output colors.
fn eval_grid(script: &crate::dsl::compiler::CompiledScript) -> Vec<Color> {
    let mut results = Vec::new();
    let test_points: &[(f64, usize, usize)] = &[
        (0.0, 0, 1),
        (0.0, 0, 10),
        (0.0, 5, 10),
        (0.0, 9, 10),
        (0.5, 0, 10),
        (0.5, 5, 10),
        (1.0, 0, 10),
        (1.0, 9, 10),
    ];
    for &(t, pixel, pixels) in test_points {
        let pos = if pixels > 1 { pixel as f64 / (pixels - 1) as f64 } else { 0.0 };
        let ctx = VmContext {
            t,
            pixel,
            pixels,
            pos,
            pos2d: (pos, 0.0),
            abs_t: 0.0,
            param_values: &[],
            gradients: &[],
            curves: &[],
            colors: &[],
            paths: &[],
        };
        results.push(execute(script, &ctx));
    }
    results
}

/// Assert that optimized and unoptimized compilation produce identical output
/// for a given source string.
fn assert_optimization_equivalence(src: &str) {
    let unopt = compile_unoptimized(src);
    let opt = compile_optimized(src);
    let unopt_results = eval_grid(&unopt);
    let opt_results = eval_grid(&opt);
    assert_eq!(
        unopt_results, opt_results,
        "Optimization changed semantics for: {src}"
    );
}

#[test]
fn opt_equivalence_constant_arithmetic() {
    // Constant folding should reduce `2.0 * 3.0` to `6.0` at compile time.
    assert_optimization_equivalence("let x = 2.0 * 3.0 / 6.0; rgb(x, x, x)");
}

#[test]
fn opt_equivalence_pi_tau_folding() {
    // PI and TAU are folded to literals by the optimizer.
    assert_optimization_equivalence("let x = sin(PI / 2.0); rgb(x, x, x)");
    assert_optimization_equivalence("let x = sin(TAU / 4.0); rgb(x, x, x)");
}

#[test]
fn opt_equivalence_negation_folding() {
    assert_optimization_equivalence("let x = -(-0.5); rgb(x, x, x)");
}

#[test]
fn opt_equivalence_boolean_folding() {
    assert_optimization_equivalence("if true { rgb(1.0, 0.0, 0.0) } else { rgb(0.0, 1.0, 0.0) }");
}

#[test]
fn opt_equivalence_complex_expression() {
    // Mix of runtime variables (t, pos) and constants — only constants should fold.
    assert_optimization_equivalence(
        "let speed = 2.0 * PI; let phase = t * speed; let x = sin(phase + pos * 360.0); let v = x * 0.5 + 0.5; rgb(v, v, v)"
    );
}

#[test]
fn opt_equivalence_conditional_with_constants() {
    assert_optimization_equivalence(
        "let threshold = 0.25 * 2.0; if t > threshold { rgb(1.0, 0.0, 0.0) } else { rgb(0.0, 0.0, 1.0) }"
    );
}

#[test]
fn opt_equivalence_int_to_float_coercion() {
    // int→float coercion of a constant should fold cleanly.
    assert_optimization_equivalence("let x = float(3) / 3.0; rgb(x, x, x)");
}

#[test]
fn opt_equivalence_nested_math() {
    // Deep nesting: triggers multiple folding passes.
    assert_optimization_equivalence(
        "let a = 1.0 + 2.0; let b = a * 3.0; let c = b - 4.0; let v = c / 5.0; rgb(v, v, v)"
    );
}

#[test]
fn opt_equivalence_user_function() {
    // User functions with constant args — the function itself isn't folded,
    // but its body's constants are.
    assert_optimization_equivalence(
        "fn scale(x: float) -> float { x * 0.5 }\nlet v = scale(t); rgb(v, v, v)"
    );
}

// ── Compile error quality ───────────────────────────────────────
// Feed broken scripts and assert the error messages/spans are correct.
// These test the compiler's external contract (error reporting), not internals.

#[test]
fn error_undefined_variable() {
    let result = crate::dsl::compile_source("rgb(undefined_var, 0.0, 0.0)");
    let errors = result.unwrap_err();
    assert!(!errors.is_empty(), "should produce at least one error");
    let msg = &errors[0].message;
    assert!(
        msg.contains("undefined") || msg.contains("undeclared") || msg.contains("not found") || msg.contains("unknown"),
        "error should mention undefined variable, got: {msg}"
    );
}

#[test]
fn error_missing_function() {
    let result = crate::dsl::compile_source("nonexistent_func(1.0, 2.0)");
    assert!(result.is_err(), "calling a nonexistent function should fail");
}

#[test]
fn error_unterminated_expression() {
    let result = crate::dsl::compile_source("rgb(1.0, 0.0,");
    assert!(result.is_err(), "unterminated expression should fail");
}

#[test]
fn error_wrong_arg_count() {
    // rgb() takes 3 args, not 2
    let result = crate::dsl::compile_source("rgb(1.0, 0.0)");
    assert!(result.is_err(), "wrong arg count should fail");
}

#[test]
fn error_return_type_mismatch() {
    // Script must return a color, not a float
    let result = crate::dsl::compile_source("1.0 + 2.0");
    assert!(result.is_err(), "script returning float instead of color should fail");
}

// ── Parameter binding stability ─────────────────────────────────
// Compile a script with multiple params, execute with specific values,
// assert the right param maps to the right slot. Catches ABI-level bugs
// where param indices silently shift.

#[test]
fn param_binding_order_preserved() {
    // Three params: r, g, b. Verify each maps to the correct slot.
    let src = "param r: float(0.0, 1.0) = 0.0;\nparam g: float(0.0, 1.0) = 0.0;\nparam b: float(0.0, 1.0) = 0.0;\nrgb(r, g, b)";
    let compiled = compile_optimized(src);

    // Verify param order in compiled output
    assert_eq!(compiled.params.len(), 3);
    assert_eq!(compiled.params[0].name, "r");
    assert_eq!(compiled.params[1].name, "g");
    assert_eq!(compiled.params[2].name, "b");

    // Execute with r=1.0, g=0.5, b=0.0
    let ctx = VmContext {
        t: 0.0, pixel: 0, pixels: 1, pos: 0.0, pos2d: (0.0, 0.0),
        param_values: &[1.0, 0.5, 0.0],
        abs_t: 0.0, gradients: &[], curves: &[], colors: &[], paths: &[],
    };
    let color = execute(&compiled, &ctx);
    assert_eq!(color.r, 255, "first param (r) should map to red channel");
    assert_eq!(color.g, 128, "second param (g) should map to green channel");
    assert_eq!(color.b, 0, "third param (b) should map to blue channel");
}

#[test]
fn param_binding_with_mixed_types() {
    // Mix of float, gradient, color params — verify indices don't cross.
    // Float params get param_values slots; gradient/color params get separate arrays.
    let src = "param speed: float(0.0, 10.0) = 1.0;\nparam palette: gradient = #000000, #ffffff;\nparam brightness: float(0.0, 1.0) = 1.0;\npalette(t) * brightness";
    let compiled = compile_optimized(src);

    assert_eq!(compiled.params.len(), 3);
    assert_eq!(compiled.params[0].name, "speed");
    assert_eq!(compiled.params[1].name, "palette");
    assert_eq!(compiled.params[2].name, "brightness");

    let gradient = crate::model::color_gradient::ColorGradient::two_color(Color::BLACK, Color::WHITE);
    // All param arrays are indexed by param position. Gradient is param index 1.
    let gradients: Vec<Option<&crate::model::color_gradient::ColorGradient>> = vec![
        None,             // slot 0: speed (float, not a gradient)
        Some(&gradient),  // slot 1: palette (gradient)
        None,             // slot 2: brightness (float, not a gradient)
    ];

    // speed=2.0, brightness=0.5 (all slots present, gradient slot unused in param_values)
    let ctx = VmContext {
        t: 0.5,
        pixel: 0,
        pixels: 1,
        pos: 0.0,
        pos2d: (0.0, 0.0),
        param_values: &[2.0, 0.0, 0.5],
        abs_t: 0.0,
        gradients: &gradients,
        curves: &[],
        colors: &[],
        paths: &[],
    };
    let color = execute(&compiled, &ctx);
    // At t=0.5, gradient gives ~mid-gray (128). Multiplied by brightness=0.5 → ~64
    assert!(
        (color.r as i16 - 64).abs() <= 2,
        "gradient(0.5) * 0.5 should be ~64, got {}",
        color.r
    );
}
