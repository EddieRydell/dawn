use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) const STARTER_SEQUENCE_PATH: &str = "sequences/example.sequence.dawn";

pub(crate) fn create_starter_project(
    parent_path: &str,
    directory_name: &str,
) -> Result<PathBuf, String> {
    validate_directory_name(directory_name)?;
    let parent = PathBuf::from(parent_path);
    if !parent.is_dir() {
        return Err("parent location is not a directory".to_string());
    }

    let target = parent.join(directory_name);
    validate_target_directory(&target)?;

    fs::create_dir_all(&target)
        .map_err(|error| format!("failed to create project directory: {error}"))?;
    for directory in STARTER_DIRECTORIES {
        fs::create_dir(target.join(directory))
            .map_err(|error| format!("failed to create {directory}: {error}"))?;
    }

    let project_name = display_name(directory_name);
    let project_key = snake_case_key(directory_name);
    let files = starter_files(&project_name, &project_key);
    for file in files {
        let path = target.join(file.path);
        if path.exists() {
            return Err(format!("starter file already exists: {}", file.path));
        }
        fs::write(&path, file.content)
            .map_err(|error| format!("failed to write {}: {error}", file.path))?;
    }

    Ok(target)
}

fn validate_directory_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("project folder name is required".to_string());
    }
    if name.trim() != name {
        return Err("project folder name cannot start or end with whitespace".to_string());
    }
    if name == "." || name == ".." {
        return Err("project folder name cannot be . or ..".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("project folder name must be a folder name, not a path".to_string());
    }
    if Path::new(name)
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("project folder name must be a plain folder name".to_string());
    }
    Ok(())
}

fn validate_target_directory(target: &Path) -> Result<(), String> {
    if target.exists() {
        if !target.is_dir() {
            return Err("target path already exists and is not a directory".to_string());
        }
        let mut entries = fs::read_dir(target)
            .map_err(|error| format!("failed to inspect target directory: {error}"))?;
        if entries
            .next()
            .transpose()
            .map_err(|error| format!("failed to inspect target directory contents: {error}"))?
            .is_some()
        {
            return Err("target directory must be empty".to_string());
        }
    }
    Ok(())
}

fn display_name(directory_name: &str) -> String {
    directory_name
        .split(|character: char| !character.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            let Some(first) = characters.next() else {
                return String::new();
            };
            let mut word = first.to_uppercase().collect::<String>();
            word.push_str(&characters.as_str().to_lowercase());
            word
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn snake_case_key(directory_name: &str) -> String {
    let mut key = String::new();
    let mut previous_was_separator = true;
    for character in directory_name.chars() {
        if character.is_ascii_alphanumeric() {
            key.push(character.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator {
            key.push('_');
            previous_was_separator = true;
        }
    }
    while key.ends_with('_') {
        key.pop();
    }
    if key.is_empty()
        || key
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
    {
        key.insert_str(0, "project_");
    }
    key
}

struct StarterFile {
    path: &'static str,
    content: String,
}

const STARTER_DIRECTORIES: &[&str] = &[
    "audio",
    "curves",
    "displays",
    "effects",
    "fixtures",
    "layouts",
    "patches",
    "sequences",
];

fn starter_files(project_name: &str, project_key: &str) -> Vec<StarterFile> {
    vec![
        StarterFile {
            path: "project.dawn",
            content: project_file(project_name, project_key),
        },
        StarterFile {
            path: "displays/main.display.dawn",
            content: display_file(project_name),
        },
        StarterFile {
            path: "layouts/main.layout.dawn",
            content: layout_file(),
        },
        StarterFile {
            path: "fixtures/basic.fixture.dawn",
            content: FIXTURE_FILE.to_string(),
        },
        StarterFile {
            path: "patches/main.patch.dawn",
            content: PATCH_FILE.to_string(),
        },
        StarterFile {
            path: "curves/basic.curve.dawn",
            content: CURVE_FILE.to_string(),
        },
        StarterFile {
            path: "effects/pulse.effect.dawn",
            content: PULSE_EFFECT.to_string(),
        },
        StarterFile {
            path: "effects/chase.effect.dawn",
            content: CHASE_EFFECT.to_string(),
        },
        StarterFile {
            path: "effects/twinkle.effect.dawn",
            content: TWINKLE_EFFECT.to_string(),
        },
        StarterFile {
            path: "effects/wipe.effect.dawn",
            content: WIPE_EFFECT.to_string(),
        },
        StarterFile {
            path: "effects/mark-pulse.effect.dawn",
            content: MARK_PULSE_EFFECT.to_string(),
        },
        StarterFile {
            path: STARTER_SEQUENCE_PATH,
            content: SEQUENCE_FILE.to_string(),
        },
        StarterFile {
            path: "AGENTS.md",
            content: AGENTS_FILE.to_string(),
        },
        StarterFile {
            path: "CLAUDE.md",
            content: CLAUDE_FILE.to_string(),
        },
    ]
}

fn project_file(project_name: &str, project_key: &str) -> String {
    format!(
        r#"imports:
  - from: displays/main.display.dawn
    as: displays
  - from: sequences/example.sequence.dawn
    as: sequences

{project_key}:
  type: project
  name: {project_name}
  display: displays.main
  sequences:
    - sequences.example
"#
    )
}

fn display_file(project_name: &str) -> String {
    format!(
        r#"imports:
  - from: ../layouts/main.layout.dawn
    as: layouts
  - from: ../patches/main.patch.dawn
    as: patches

main:
  type: display
  name: {project_name}
  layout: layouts.main
  patch: patches.main
  controllers:
    - name: OutputController
      protocol: sacn
      output:
        type: linear_rgb
        channel_order: grb
        group: All Outputs
        output_count: 30
        pixels_per_output: 113
        first_universe: 1
        slots_per_universe: 510
"#
    )
}

fn layout_file() -> String {
    let mut content = String::from(
        r#"imports:
  - from: ../fixtures/basic.fixture.dawn
    as: fixtures

main:
  type: layout
  name: 30 Output Lines
  target_order:
    - type: group
      name: All Outputs
"#,
    );
    for output in 1..=30 {
        content.push_str(&format!(
            "    - type: fixture\n      name: Output {output:02}\n"
        ));
    }
    content.push_str("  fixtures:\n");
    for output in 1..=30 {
        let x = (output - 1) as f64 * 0.35;
        content.push_str(&format!(
            "    - id: {output}\n      name: Output {output:02}\n      fixture: fixtures.vertical_113\n      transform:\n        position: {{ x: {x:.2}, y: 0.0, z: 0.0 }}\n"
        ));
    }
    content.push_str("  groups:\n    - name: All Outputs\n      members:\n");
    for output in 1..=30 {
        content.push_str(&format!("        - {output}\n"));
    }
    content
}

const FIXTURE_FILE: &str = r#"vertical_113:
  type: fixture
  name: Vertical 113px Line
  color_model: rgb
  bulb_diameter: 0.05
  geometry:
    type: lines
    points:
      - { x: 0.0, y: 0.0, z: 0.0 }
      - { x: 0.0, y: 11.2, z: 0.0 }
    pixels: 113

horizontal_50:
  type: fixture
  name: Horizontal 50px Line
  color_model: rgb
  bulb_diameter: 0.05
  geometry:
    type: lines
    points:
      - { x: 0.0, y: 0.0, z: 0.0 }
      - { x: 4.9, y: 0.0, z: 0.0 }
    pixels: 50

pixel_bar_100:
  type: fixture
  name: Pixel Bar 100px
  color_model: rgb
  bulb_diameter: 0.045
  geometry:
    type: lines
    points:
      - { x: -5.0, y: 0.0, z: 0.0 }
      - { x: 5.0, y: 0.0, z: 0.0 }
    pixels: 100

single_pixel:
  type: fixture
  name: Single Pixel
  color_model: rgb
  bulb_diameter: 0.08
  geometry:
    type: points
    points:
      - { x: 0.0, y: 0.0, z: 0.0 }

two_eye_blinder:
  type: fixture
  name: Two Eye Blinder
  color_model: rgb
  bulb_diameter: 0.12
  geometry:
    type: points
    points:
      - { x: -0.35, y: 0.0, z: 0.0 }
      - { x: 0.35, y: 0.0, z: 0.0 }

grid_4x4:
  type: fixture
  name: 4x4 Pixel Grid
  color_model: rgb
  bulb_diameter: 0.055
  geometry:
    type: points
    points:
      - { x: -0.6, y: 0.6, z: 0.0 }
      - { x: -0.2, y: 0.6, z: 0.0 }
      - { x: 0.2, y: 0.6, z: 0.0 }
      - { x: 0.6, y: 0.6, z: 0.0 }
      - { x: -0.6, y: 0.2, z: 0.0 }
      - { x: -0.2, y: 0.2, z: 0.0 }
      - { x: 0.2, y: 0.2, z: 0.0 }
      - { x: 0.6, y: 0.2, z: 0.0 }
      - { x: -0.6, y: -0.2, z: 0.0 }
      - { x: -0.2, y: -0.2, z: 0.0 }
      - { x: 0.2, y: -0.2, z: 0.0 }
      - { x: 0.6, y: -0.2, z: 0.0 }
      - { x: -0.6, y: -0.6, z: 0.0 }
      - { x: -0.2, y: -0.6, z: 0.0 }
      - { x: 0.2, y: -0.6, z: 0.0 }
      - { x: 0.6, y: -0.6, z: 0.0 }

rectangle_96:
  type: fixture
  name: Rectangle Outline 96px
  color_model: rgb
  bulb_diameter: 0.05
  geometry:
    type: lines
    points:
      - { x: -1.2, y: 0.75, z: 0.0 }
      - { x: 1.2, y: 0.75, z: 0.0 }
      - { x: 1.2, y: -0.75, z: 0.0 }
      - { x: -1.2, y: -0.75, z: 0.0 }
      - { x: -1.2, y: 0.75, z: 0.0 }
    pixels: 96

triangle_75:
  type: fixture
  name: Triangle Outline 75px
  color_model: rgb
  bulb_diameter: 0.05
  geometry:
    type: lines
    points:
      - { x: -1.0, y: -0.6, z: 0.0 }
      - { x: 0.0, y: 1.1, z: 0.0 }
      - { x: 1.0, y: -0.6, z: 0.0 }
      - { x: -1.0, y: -0.6, z: 0.0 }
    pixels: 75

half_arc_60:
  type: fixture
  name: Half Arc 60px
  color_model: rgb
  bulb_diameter: 0.05
  geometry:
    type: arc
    center: { x: 0.0, y: 0.0, z: 0.0 }
    radius: 1.2
    startDegrees: 0.0
    endDegrees: 180.0
    pixels: 60

circle_72:
  type: fixture
  name: Circle 72px
  color_model: rgb
  bulb_diameter: 0.05
  geometry:
    type: arc
    center: { x: 0.0, y: 0.0, z: 0.0 }
    radius: 1.0
    startDegrees: 0.0
    endDegrees: 360.0
    pixels: 72

tree_outline_88:
  type: fixture
  name: Tree Outline 88px
  color_model: rgb
  bulb_diameter: 0.05
  geometry:
    type: lines
    points:
      - { x: 0.0, y: 1.25, z: 0.0 }
      - { x: -0.9, y: -0.25, z: 0.0 }
      - { x: -0.45, y: -0.25, z: 0.0 }
      - { x: -1.1, y: -1.15, z: 0.0 }
      - { x: 1.1, y: -1.15, z: 0.0 }
      - { x: 0.45, y: -0.25, z: 0.0 }
      - { x: 0.9, y: -0.25, z: 0.0 }
      - { x: 0.0, y: 1.25, z: 0.0 }
    pixels: 88

star_10:
  type: fixture
  name: Star 10 Points
  color_model: rgb
  bulb_diameter: 0.07
  geometry:
    type: points
    points:
      - { x: 0.0, y: 0.55, z: 0.0 }
      - { x: 0.16, y: 0.16, z: 0.0 }
      - { x: 0.55, y: 0.16, z: 0.0 }
      - { x: 0.24, y: -0.08, z: 0.0 }
      - { x: 0.34, y: -0.48, z: 0.0 }
      - { x: 0.0, y: -0.24, z: 0.0 }
      - { x: -0.34, y: -0.48, z: 0.0 }
      - { x: -0.24, y: -0.08, z: 0.0 }
      - { x: -0.55, y: 0.16, z: 0.0 }
      - { x: -0.16, y: 0.16, z: 0.0 }
"#;

const PATCH_FILE: &str = r#"main:
  type: patch
"#;

const CURVE_FILE: &str = r#"linear_up:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 0.0
    - time: 1.0
      value: 1.0

linear_down:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 1.0
    - time: 1.0
      value: 0.0

ease_up:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 0.0
    - time: 0.125
      value: 0.234
    - time: 0.25
      value: 0.438
    - time: 0.375
      value: 0.609
    - time: 0.5
      value: 0.75
    - time: 0.625
      value: 0.859
    - time: 0.75
      value: 0.938
    - time: 0.875
      value: 0.984
    - time: 1.0
      value: 1.0

ease_down:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 1.0
    - time: 0.125
      value: 0.766
    - time: 0.25
      value: 0.563
    - time: 0.375
      value: 0.391
    - time: 0.5
      value: 0.25
    - time: 0.625
      value: 0.141
    - time: 0.75
      value: 0.063
    - time: 0.875
      value: 0.016
    - time: 1.0
      value: 0.0

ease_up_in:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 0.0
    - time: 0.125
      value: 0.016
    - time: 0.25
      value: 0.063
    - time: 0.375
      value: 0.141
    - time: 0.5
      value: 0.25
    - time: 0.625
      value: 0.391
    - time: 0.75
      value: 0.563
    - time: 0.875
      value: 0.766
    - time: 1.0
      value: 1.0

ease_down_in:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 1.0
    - time: 0.125
      value: 0.984
    - time: 0.25
      value: 0.938
    - time: 0.375
      value: 0.859
    - time: 0.5
      value: 0.75
    - time: 0.625
      value: 0.609
    - time: 0.75
      value: 0.438
    - time: 0.875
      value: 0.234
    - time: 1.0
      value: 0.0

soft_pulse:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 0.0
    - time: 0.15
      value: 0.65
    - time: 0.32
      value: 1.0
    - time: 0.6
      value: 0.42
    - time: 1.0
      value: 0.0

hard_pulse:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 1.0
    - time: 0.08
      value: 1.0
    - time: 0.22
      value: 0.45
    - time: 1.0
      value: 0.0

center_peak:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 0.0
    - time: 0.25
      value: 0.5
    - time: 0.5
      value: 1.0
    - time: 0.75
      value: 0.5
    - time: 1.0
      value: 0.0

hold_then_drop:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 1.0
    - time: 0.6
      value: 1.0
    - time: 0.78
      value: 0.35
    - time: 1.0
      value: 0.0

drop_then_hold:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 1.0
    - time: 0.18
      value: 0.35
    - time: 0.4
      value: 0.0
    - time: 1.0
      value: 0.0

hue_sweep:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 0.0
    - time: 0.2
      value: 55.0
    - time: 0.4
      value: 130.0
    - time: 0.65
      value: 220.0
    - time: 0.85
      value: 300.0
    - time: 1.0
      value: 360.0

hue_warm_shift:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 18.0
    - time: 0.35
      value: 38.0
    - time: 0.7
      value: 350.0
    - time: 1.0
      value: 330.0

hue_cool_shift:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 185.0
    - time: 0.35
      value: 210.0
    - time: 0.7
      value: 255.0
    - time: 1.0
      value: 285.0

hue_ping_pong:
  type: curve
  value_type: float
  points:
    - time: 0.0
      value: 300.0
    - time: 0.25
      value: 210.0
    - time: 0.5
      value: 120.0
    - time: 0.75
      value: 210.0
    - time: 1.0
      value: 300.0

warm_gradient:
  type: curve
  value_type: color
  points:
    - time: 0.0
      value: '#fff4d6'
    - time: 0.35
      value: '#ffb000'
    - time: 1.0
      value: '#ff2a00'

cool_gradient:
  type: curve
  value_type: color
  points:
    - time: 0.0
      value: '#d9ffff'
    - time: 0.45
      value: '#00a6ff'
    - time: 1.0
      value: '#3d00ff'

club_gradient:
  type: curve
  value_type: color
  points:
    - time: 0.0
      value: '#ffffff'
    - time: 0.25
      value: '#ff2bd6'
    - time: 0.6
      value: '#2de2ff'
    - time: 1.0
      value: '#7cff00'

fire_gradient:
  type: curve
  value_type: color
  points:
    - time: 0.0
      value: '#ffffff'
    - time: 0.18
      value: '#ffd15c'
    - time: 0.55
      value: '#ff4a00'
    - time: 1.0
      value: '#1a0000'
"#;

const PULSE_EFFECT: &str = r#"effect Pulse {
  param curve<color> gradient;
  param curve<float> pulse_shape;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    return gradient(progress) * pulse_shape(progress);
  }
}
"#;

const CHASE_EFFECT: &str = r#"effect Chase {
  param curve<color> gradient;
  param enum gradient_mode { through_effect, across_items, per_pulse } = through_effect;
  param float pulse_overlap = 8.0;
  param curve<float> chase_position;
  param bool extend_to_start = false;
  param bool extend_to_end = false;
  param curve<float> pulse_shape;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    float pixel_position = 0.0;
    if (pixel_count(pixel) > 1.0) {
      pixel_position = pixel_index(pixel) / (pixel_count(pixel) - 1.0);
    }

    float pixel_step = 1.0 / max(1.0, pixel_count(pixel) - 1.0);
    float pulse_radius = max(0.5, pulse_overlap) * pixel_step;
    float travel_start = 0.0;
    float travel_end = 1.0;
    if (extend_to_start) {
      travel_start = travel_start - pulse_radius;
    }
    if (extend_to_end) {
      travel_end = travel_end + pulse_radius;
    }

    float chase = mix(travel_start, travel_end, clamp(chase_position(progress), 0.0, 1.0));
    float distance = abs(pixel_position - chase);
    float level = 0.0;
    if (distance <= pulse_radius) {
      float pulse_progress = clamp((chase + pulse_radius - pixel_position) / max(0.000000001, pulse_radius * 2.0), 0.0, 1.0);
      level = clamp(pulse_shape(pulse_progress), 0.0, 1.0);
    }

    float gradient_position = progress;
    if (gradient_mode == across_items) {
      gradient_position = pixel_position;
    }
    if (gradient_mode == per_pulse) {
      gradient_position = clamp((chase + pulse_radius - pixel_position) / max(0.000000001, pulse_radius * 2.0), 0.0, 1.0);
    }

    return gradient(clamp(gradient_position, 0.0, 1.0)) * level;
  }
}
"#;

const TWINKLE_EFFECT: &str = r#"effect Twinkle {
  param color base = #000000;
  param color sparkle = #ffffff;
  param float density = 0.32;
  param float speed = 1.4;
  param float seed = 0.0;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    float bucket = floor(seconds * speed + pixel_index(pixel) * 0.37);
    srand(seed + pixel_index(pixel) * 17.0 + bucket * 23.0);
    float on = rand();
    float shimmer = (sin(seconds * 9.0 + pixel_index(pixel)) + 1.0) / 2.0;
    if (on < density) {
      return mix(base, sparkle, shimmer);
    }
    return base;
  }
}
"#;

const WIPE_EFFECT: &str = r#"effect Wipe {
  param curve<color> gradient;
  param curve<float> position;
  param float edge_width = 0.08;
  param bool reverse = false;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    float pixel_position = 0.0;
    if (pixel_count(pixel) > 1.0) {
      pixel_position = pixel_index(pixel) / (pixel_count(pixel) - 1.0);
    }
    if (reverse) {
      pixel_position = 1.0 - pixel_position;
    }

    float wipe = clamp(position(progress), 0.0, 1.0);
    float edge = max(0.000000001, edge_width);
    float level = clamp((wipe - pixel_position + edge) / edge, 0.0, 1.0);
    return gradient(pixel_position) * level;
  }
}
"#;

const MARK_PULSE_EFFECT: &str = r#"effect MarkPulse {
  param marks beats;
  param color base = #000000;
  param curve<color> accent;
  param curve<float> hue;
  param float hue_mix = 0.35;
  param float offset_seconds = 0.0;
  param float decay_seconds = 0.18;
  param int section_width_pixels = 5;
  param float section_edge_fade_pixels = 0.0;
  param int sections_per_mark = 3;
  param float seed = 0.0;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    float query_time = seconds - offset_seconds;
    float previous = mark_prev(beats, query_time, -999.0);
    float elapsed = mark_elapsed(beats, query_time, 999.0);
    float width = max(1.0, section_width_pixels);
    float pixel_position = pixel_index(pixel);
    float section = floor(pixel_position / width);
    float section_count = max(1.0, pixel_count(pixel) / width);
    float edge_fade = max(0.0, section_edge_fade_pixels);
    float active = 0.0;
    srand(seed + previous * 1000.0);

    for (int i = 0; i < sections_per_mark; i = i + 1) {
      float choice = floor(rand() * section_count);
      float hit = 0.0;
      if (section == choice) {
        hit = 1.0;
        if (edge_fade > 0.0) {
          float section_start = choice * width;
          float section_end = min(section_start + width - 1.0, pixel_count(pixel) - 1.0);
          float edge_distance = min(pixel_position - section_start, section_end - pixel_position);
          hit = clamp(edge_distance / edge_fade, 0.0, 1.0);
        }
      }
      active = max(active, hit);
    }

    float pulse_progress = clamp(elapsed / max(0.000000001, decay_seconds), 0.0, 1.0);
    float pulse = 1.0 - pulse_progress;
    color pulse_color = mix(accent(pulse_progress), hsv(hue(progress), 1.0, 1.0), clamp(hue_mix, 0.0, 1.0));
    return mix(base, pulse_color, active * pulse);
  }
}
"#;

const SEQUENCE_FILE: &str = r#"imports:
  - from: ../effects
    as: effects
  - from: ../curves
    as: curves

example:
  type: sequence
  duration: 120s
  frame_rate: 144
  mark_collections:
    - key: marks
      name: Marks
      color: '#38bdf8'
      marks: []
  effects: []
  automation_clips: []
"#;

const AGENTS_FILE: &str = r#"# Repository Guidelines

This is a Dawn project. Keep authored project files in `project.dawn`, `displays/`, `layouts/`, `fixtures/`, `patches/`, `curves/`, `effects/`, and `sequences/`.

Use existing curves and effect scripts before adding new ones. Do not add generated files, compatibility shims, hidden fallbacks, or destructive cleanup. Never overwrite user-authored show files without inspecting them first.

Validate edits with Dawn tooling from the Dawn workspace, for example `cargo run -p dawn-cli -- analyze <project-folder>`.
"#;

const CLAUDE_FILE: &str = r#"# Claude Instructions

This is a Dawn lighting project. Preserve the folder structure and imports in `project.dawn`.

Prefer editing existing curves, effects, layouts, fixtures, patches, and sequences. Avoid generated assets, destructive changes, compatibility layers, and silent fallbacks. Validate project changes with Dawn analyze/check commands before handing work back.
"#;
