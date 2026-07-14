use indexmap::{IndexMap, IndexSet};

use crate::element::IndexedOptionId;
use crate::identity::SourceIdentity;
use crate::values::{Color, Curve};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FixtureProfileId(pub SourceIdentity);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct FixtureFunctionId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct FixtureEntryId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureProfile {
    pub id: FixtureProfileId,
    pub functions: IndexMap<FixtureFunctionId, FixtureFunction>,
    pub channels: Vec<FixtureChannel>,
    pub behavior_rules: Vec<FixtureBehaviorRule>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureFunction {
    pub name: String,
    pub tag: Option<FixtureFunctionTag>,
    pub kind: FixtureFunctionKind,
    pub curve: DimmingCurve,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum FixtureFunctionTag {
    Pan,
    Tilt,
    Dimmer,
    Shutter,
    Zoom,
    Gobo,
    Frost,
    Prism,
    ColorWheel,
    ColorMixing,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FixtureFunctionKind {
    Range,
    Indexed { entries: Vec<FixtureIndexedEntry> },
    ColorWheel { entries: Vec<FixtureIndexedEntry> },
    ColorMixing { model: ColorMixingModel },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorMixingModel {
    Rgb,
    Rgbw,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureIndexedEntry {
    pub id: FixtureEntryId,
    pub name: String,
    pub dmx_min: u16,
    pub dmx_max: u16,
    pub curve_control: bool,
    pub color: Option<Color>,
    pub tag: Option<FixtureEntryTag>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum FixtureEntryTag {
    ShutterOpen,
    ShutterClosed,
    Strobe,
    PrismOpen,
    PrismClosed,
    GoboOpen,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DimmingCurve {
    Linear,
    Gamma(f64),
    Custom(Curve),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureChannel {
    pub slot: u16,
    pub role: FixtureChannelRole,
    pub curve: DimmingCurve,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureChannelRole {
    Coarse {
        function: FixtureFunctionId,
    },
    Fine {
        function: FixtureFunctionId,
    },
    ColorComponent {
        function: FixtureFunctionId,
        component: ColorComponent,
    },
    Ignored,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ColorComponent {
    Red,
    Green,
    Blue,
    White,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FixtureBehaviorRule {
    Shutter {
        function: FixtureFunctionId,
        closed: FixtureEntryId,
        open: FixtureEntryId,
    },
    Dimmer {
        function: FixtureFunctionId,
        off: f64,
        on: f64,
    },
    ColorWheel {
        function: FixtureFunctionId,
        entries: Vec<ColorWheelColorMapping>,
    },
    PrismGate {
        function: FixtureFunctionId,
        disabled: FixtureEntryId,
        enabled: FixtureEntryId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColorWheelColorMapping {
    pub color: Color,
    pub entry: FixtureEntryId,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FixtureControlValue {
    Normalized(f64),
    Indexed { entry: FixtureEntryId, range: f64 },
    Color(Color),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FixtureState {
    pub functions: IndexMap<FixtureFunctionId, FixtureControlValue>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FixtureProfileStore {
    pub definitions: IndexMap<FixtureProfileId, FixtureProfile>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FixtureProfileValidationError {
    EmptyFunctions,
    DuplicateChannelSlot(u16),
    MissingFunction(FixtureFunctionId),
    DuplicateEntry {
        function: FixtureFunctionId,
        entry: FixtureEntryId,
    },
    InvalidEntryRange {
        function: FixtureFunctionId,
        entry: FixtureEntryId,
    },
    OverlappingEntryRanges {
        function: FixtureFunctionId,
    },
    MissingEntry {
        function: FixtureFunctionId,
        entry: FixtureEntryId,
    },
    InvalidCurve,
    MissingCoarseChannel(FixtureFunctionId),
    DuplicateCoarseChannel(FixtureFunctionId),
    DuplicateFineChannel(FixtureFunctionId),
    FineWithoutCoarse(FixtureFunctionId),
    MissingColorComponent {
        function: FixtureFunctionId,
        component: ColorComponent,
    },
    DuplicateColorComponent {
        function: FixtureFunctionId,
        component: ColorComponent,
    },
    UnexpectedColorComponent(FixtureFunctionId),
    DuplicateBehaviorFunction(FixtureFunctionId),
    DuplicateBehaviorColor {
        function: FixtureFunctionId,
        color: Color,
    },
    DuplicateBehaviorEntry {
        function: FixtureFunctionId,
        entry: FixtureEntryId,
    },
    InvalidBehaviorValue,
}

impl FixtureProfile {
    pub fn validate(&self) -> Result<(), FixtureProfileValidationError> {
        if self.functions.is_empty() {
            return Err(FixtureProfileValidationError::EmptyFunctions);
        }
        for (id, function) in &self.functions {
            validate_curve(&function.curve)?;
            if let FixtureFunctionKind::Indexed { entries }
            | FixtureFunctionKind::ColorWheel { entries } = &function.kind
            {
                validate_entries(*id, entries)?;
            }
        }

        let mut slots = IndexSet::new();
        let mut coarse = IndexSet::new();
        let mut fine = IndexSet::new();
        let mut components: IndexMap<FixtureFunctionId, IndexSet<ColorComponent>> = IndexMap::new();
        for channel in &self.channels {
            if !slots.insert(channel.slot) {
                return Err(FixtureProfileValidationError::DuplicateChannelSlot(
                    channel.slot,
                ));
            }
            validate_curve(&channel.curve)?;
            let function = match channel.role {
                FixtureChannelRole::Coarse { function }
                | FixtureChannelRole::Fine { function }
                | FixtureChannelRole::ColorComponent { function, .. } => Some(function),
                FixtureChannelRole::Ignored => None,
            };
            if let Some(function) = function
                && !self.functions.contains_key(&function)
            {
                return Err(FixtureProfileValidationError::MissingFunction(function));
            }
            match channel.role {
                FixtureChannelRole::Coarse { function } if !coarse.insert(function) => {
                    return Err(FixtureProfileValidationError::DuplicateCoarseChannel(
                        function,
                    ));
                }
                FixtureChannelRole::Fine { function } if !fine.insert(function) => {
                    return Err(FixtureProfileValidationError::DuplicateFineChannel(
                        function,
                    ));
                }
                FixtureChannelRole::ColorComponent {
                    function,
                    component,
                } if !components.entry(function).or_default().insert(component) => {
                    return Err(FixtureProfileValidationError::DuplicateColorComponent {
                        function,
                        component,
                    });
                }
                _ => {}
            }
        }
        for (id, function) in &self.functions {
            match &function.kind {
                FixtureFunctionKind::ColorMixing { model } => {
                    let present = components.get(id).cloned().unwrap_or_default();
                    for component in [
                        ColorComponent::Red,
                        ColorComponent::Green,
                        ColorComponent::Blue,
                    ] {
                        if !present.contains(&component) {
                            return Err(FixtureProfileValidationError::MissingColorComponent {
                                function: *id,
                                component,
                            });
                        }
                    }
                    if *model == ColorMixingModel::Rgbw && !present.contains(&ColorComponent::White)
                    {
                        return Err(FixtureProfileValidationError::MissingColorComponent {
                            function: *id,
                            component: ColorComponent::White,
                        });
                    }
                }
                _ if components.contains_key(id) => {
                    return Err(FixtureProfileValidationError::UnexpectedColorComponent(*id));
                }
                _ => {
                    if !coarse.contains(id) {
                        return Err(FixtureProfileValidationError::MissingCoarseChannel(*id));
                    }
                }
            }
            if fine.contains(id) && !coarse.contains(id) {
                return Err(FixtureProfileValidationError::FineWithoutCoarse(*id));
            }
        }
        let mut behavior_functions = IndexSet::new();
        for rule in &self.behavior_rules {
            let function = behavior_function(rule);
            if !behavior_functions.insert(function) {
                return Err(FixtureProfileValidationError::DuplicateBehaviorFunction(
                    function,
                ));
            }
            validate_rule(self, rule)?;
        }
        Ok(())
    }

    pub fn slot_count(&self) -> usize {
        self.channels
            .iter()
            .map(|channel| usize::from(channel.slot) + 1)
            .max()
            .unwrap_or(0)
    }
}

fn validate_curve(curve: &DimmingCurve) -> Result<(), FixtureProfileValidationError> {
    match curve {
        DimmingCurve::Linear => Ok(()),
        DimmingCurve::Gamma(value) if value.is_finite() && *value > 0.0 => Ok(()),
        DimmingCurve::Custom(curve)
            if !curve.points.is_empty()
                && curve.points.iter().all(|point| {
                    point.position.is_finite()
                        && point.value.is_finite()
                        && (0.0..=1.0).contains(&point.position)
                        && (0.0..=1.0).contains(&point.value)
                }) =>
        {
            Ok(())
        }
        _ => Err(FixtureProfileValidationError::InvalidCurve),
    }
}

fn validate_entries(
    function: FixtureFunctionId,
    entries: &[FixtureIndexedEntry],
) -> Result<(), FixtureProfileValidationError> {
    let mut ids = IndexSet::new();
    for entry in entries {
        if !ids.insert(entry.id) {
            return Err(FixtureProfileValidationError::DuplicateEntry {
                function,
                entry: entry.id,
            });
        }
        if entry.dmx_min > entry.dmx_max {
            return Err(FixtureProfileValidationError::InvalidEntryRange {
                function,
                entry: entry.id,
            });
        }
    }
    let mut ranges = entries
        .iter()
        .map(|entry| (entry.dmx_min, entry.dmx_max))
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    if ranges.windows(2).any(|pair| pair[0].1 >= pair[1].0) {
        return Err(FixtureProfileValidationError::OverlappingEntryRanges { function });
    }
    Ok(())
}

fn validate_rule(
    profile: &FixtureProfile,
    rule: &FixtureBehaviorRule,
) -> Result<(), FixtureProfileValidationError> {
    let (function, entries): (FixtureFunctionId, Vec<FixtureEntryId>) = match rule {
        FixtureBehaviorRule::Shutter {
            function,
            closed,
            open,
        } => (*function, vec![*closed, *open]),
        FixtureBehaviorRule::Dimmer { function, off, on } => {
            if !off.is_finite()
                || !on.is_finite()
                || !(0.0..=1.0).contains(off)
                || !(0.0..=1.0).contains(on)
            {
                return Err(FixtureProfileValidationError::InvalidBehaviorValue);
            }
            (*function, Vec::new())
        }
        FixtureBehaviorRule::ColorWheel { function, entries } => {
            let mut colors = IndexSet::new();
            let mut mapped_entries = IndexSet::new();
            for mapping in entries {
                if !colors.insert(mapping.color) {
                    return Err(FixtureProfileValidationError::DuplicateBehaviorColor {
                        function: *function,
                        color: mapping.color,
                    });
                }
                if !mapped_entries.insert(mapping.entry) {
                    return Err(FixtureProfileValidationError::DuplicateBehaviorEntry {
                        function: *function,
                        entry: mapping.entry,
                    });
                }
            }
            (*function, entries.iter().map(|entry| entry.entry).collect())
        }
        FixtureBehaviorRule::PrismGate {
            function,
            disabled,
            enabled,
        } => (*function, vec![*disabled, *enabled]),
    };
    let definition = profile
        .functions
        .get(&function)
        .ok_or(FixtureProfileValidationError::MissingFunction(function))?;
    let available = match &definition.kind {
        FixtureFunctionKind::Indexed { entries } | FixtureFunctionKind::ColorWheel { entries } => {
            entries
                .iter()
                .map(|entry| entry.id)
                .collect::<IndexSet<_>>()
        }
        _ => IndexSet::new(),
    };
    for entry in entries {
        if !available.contains(&entry) {
            return Err(FixtureProfileValidationError::MissingEntry { function, entry });
        }
    }
    Ok(())
}

fn behavior_function(rule: &FixtureBehaviorRule) -> FixtureFunctionId {
    match rule {
        FixtureBehaviorRule::Shutter { function, .. }
        | FixtureBehaviorRule::Dimmer { function, .. }
        | FixtureBehaviorRule::ColorWheel { function, .. }
        | FixtureBehaviorRule::PrismGate { function, .. } => *function,
    }
}

pub fn indexed_option_to_fixture_entry(option: IndexedOptionId) -> FixtureEntryId {
    FixtureEntryId(option.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color(red: u8, green: u8, blue: u8) -> Color {
        Color { red, green, blue }
    }

    fn color_wheel_profile(behavior_rules: Vec<FixtureBehaviorRule>) -> FixtureProfile {
        let function = FixtureFunctionId(1);
        FixtureProfile {
            id: FixtureProfileId(SourceIdentity::new(
                "fixtures/profile.fixture.dawn".into(),
                "profile".to_string(),
            )),
            functions: IndexMap::from([(
                function,
                FixtureFunction {
                    name: "Color wheel".to_string(),
                    tag: Some(FixtureFunctionTag::ColorWheel),
                    kind: FixtureFunctionKind::ColorWheel {
                        entries: vec![
                            FixtureIndexedEntry {
                                id: FixtureEntryId(1),
                                name: "Red".to_string(),
                                dmx_min: 0,
                                dmx_max: 127,
                                curve_control: false,
                                color: Some(color(255, 0, 0)),
                                tag: None,
                            },
                            FixtureIndexedEntry {
                                id: FixtureEntryId(2),
                                name: "Blue".to_string(),
                                dmx_min: 128,
                                dmx_max: 255,
                                curve_control: false,
                                color: Some(color(0, 0, 255)),
                                tag: None,
                            },
                        ],
                    },
                    curve: DimmingCurve::Linear,
                },
            )]),
            channels: vec![FixtureChannel {
                slot: 0,
                role: FixtureChannelRole::Coarse { function },
                curve: DimmingCurve::Linear,
            }],
            behavior_rules,
        }
    }

    #[test]
    fn rejects_duplicate_behavior_functions() {
        let function = FixtureFunctionId(1);
        let profile = color_wheel_profile(vec![
            FixtureBehaviorRule::ColorWheel {
                function,
                entries: Vec::new(),
            },
            FixtureBehaviorRule::ColorWheel {
                function,
                entries: Vec::new(),
            },
        ]);

        assert_eq!(
            profile.validate(),
            Err(FixtureProfileValidationError::DuplicateBehaviorFunction(
                function
            ))
        );
    }

    #[test]
    fn rejects_duplicate_color_wheel_colors() {
        let function = FixtureFunctionId(1);
        let red = color(255, 0, 0);
        let profile = color_wheel_profile(vec![FixtureBehaviorRule::ColorWheel {
            function,
            entries: vec![
                ColorWheelColorMapping {
                    color: red,
                    entry: FixtureEntryId(1),
                },
                ColorWheelColorMapping {
                    color: red,
                    entry: FixtureEntryId(2),
                },
            ],
        }]);

        assert_eq!(
            profile.validate(),
            Err(FixtureProfileValidationError::DuplicateBehaviorColor {
                function,
                color: red,
            })
        );
    }

    #[test]
    fn rejects_duplicate_color_wheel_entries() {
        let function = FixtureFunctionId(1);
        let entry = FixtureEntryId(1);
        let profile = color_wheel_profile(vec![FixtureBehaviorRule::ColorWheel {
            function,
            entries: vec![
                ColorWheelColorMapping {
                    color: color(255, 0, 0),
                    entry,
                },
                ColorWheelColorMapping {
                    color: color(0, 0, 255),
                    entry,
                },
            ],
        }]);

        assert_eq!(
            profile.validate(),
            Err(FixtureProfileValidationError::DuplicateBehaviorEntry { function, entry })
        );
    }
}
