use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use dawn_language::element::{ColorCapability, DiscreteColorMapping, DiscreteEmitter, EmitterId};
use dawn_language::fixture_profile::*;
use dawn_language::identity::{DocumentId, SourceIdentity};
use dawn_language::patch::{FilterDefinition, prepare_filter, prepare_fixture_encoding};
use dawn_language::values::Color;
use dawn_runtime::element::{ElementNodeId, RenderedElementState};
use dawn_runtime::fixture::FixtureEncodingError;
use dawn_runtime::patch::{
    FilterError, PatchSource, PatchSourceCell, PatchStep, PatchValue, PatchValueLayout,
    PreparedPatch,
};
use indexmap::IndexMap;

struct CountingAllocator;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

fn source(output: u32, element: u32, cells: u32) -> PatchStep {
    PatchStep::Source {
        output,
        source: PatchSource {
            cells: (0..cells)
                .map(|cell| PatchSourceCell { element, cell })
                .collect(),
        },
    }
}

#[test]
fn prepared_filters_and_fixture_channels_are_exact_and_do_not_allocate() {
    let color = Color {
        red: 128,
        green: 64,
        blue: 32,
    };
    let discrete = prepare_filter(&FilterDefinition::ColorBreakdown {
        capability: ColorCapability::Discrete {
            emitters: [2, 9]
                .map(|id| DiscreteEmitter {
                    id: EmitterId(id),
                    name: id.to_string(),
                })
                .into(),
            mappings: vec![
                DiscreteColorMapping {
                    color,
                    levels: IndexMap::from([(EmitterId(9), 0.25)]),
                },
                DiscreteColorMapping {
                    color,
                    levels: IndexMap::from([(EmitterId(2), 1.0)]),
                },
            ],
        },
        cell_count: 2,
    })
    .unwrap();
    let indexed = prepare_filter(&FilterDefinition::IndexedValueMapping {
        entries: IndexMap::from([(50, 0.8), (2, 0.2)]),
        width: 2,
    })
    .unwrap();
    let range = FixtureFunctionId(9);
    let wheel = FixtureFunctionId(3);
    let mixing = FixtureFunctionId(7);
    let mut profile = FixtureProfile {
        id: FixtureProfileId(SourceIdentity::from_document(
            DocumentId::new(uuid::Uuid::from_u128(2), "fixture.dawn".into()),
            "fixture".into(),
        )),
        functions: IndexMap::from([
            (
                range,
                FixtureFunction {
                    name: "range".into(),
                    tag: None,
                    kind: FixtureFunctionKind::Range,
                    curve: DimmingCurve::Gamma(2.0),
                },
            ),
            (
                wheel,
                FixtureFunction {
                    name: "wheel".into(),
                    tag: None,
                    curve: DimmingCurve::Linear,
                    kind: FixtureFunctionKind::Indexed {
                        entries: [(90, 100, 200), (2, 0, 99)]
                            .map(|(id, min, max)| FixtureIndexedEntry {
                                id: FixtureEntryId(id),
                                name: id.to_string(),
                                dmx_min: min,
                                dmx_max: max,
                                curve_control: true,
                                color: None,
                                tag: None,
                            })
                            .into(),
                    },
                },
            ),
            (
                mixing,
                FixtureFunction {
                    name: "mix".into(),
                    tag: None,
                    kind: FixtureFunctionKind::ColorMixing {
                        model: ColorMixingModel::Rgbw,
                    },
                    curve: DimmingCurve::Linear,
                },
            ),
        ]),
        channels: vec![
            FixtureChannel {
                slot: 0,
                role: FixtureChannelRole::Fine { function: range },
                curve: DimmingCurve::Linear,
            },
            FixtureChannel {
                slot: 1,
                role: FixtureChannelRole::Coarse { function: range },
                curve: DimmingCurve::Linear,
            },
            FixtureChannel {
                slot: 2,
                role: FixtureChannelRole::Coarse { function: wheel },
                curve: DimmingCurve::Gamma(2.0),
            },
        ],
        behavior_rules: vec![],
    };
    profile.channels.extend(
        [
            ColorComponent::Red,
            ColorComponent::Green,
            ColorComponent::Blue,
            ColorComponent::White,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, component)| FixtureChannel {
            slot: 3 + index as u16,
            role: FixtureChannelRole::ColorComponent {
                function: mixing,
                component,
            },
            curve: DimmingCurve::Linear,
        }),
    );
    profile.channels.push(FixtureChannel {
        slot: 7,
        role: FixtureChannelRole::Ignored,
        curve: DimmingCurve::Linear,
    });
    profile.validate().unwrap();
    let program = prepare_fixture_encoding(&profile, 8).unwrap();
    let state = FixtureState {
        functions: vec![
            (mixing, FixtureControlValue::Color(color)),
            (
                wheel,
                FixtureControlValue::Indexed {
                    entry: FixtureEntryId(90),
                    range: 0.5,
                },
            ),
            (range, FixtureControlValue::Normalized(0.5)),
        ],
    };
    let elements = vec![
        RenderedElementState::Color {
            node: ElementNodeId(1),
            cells: vec![color; 2],
        },
        RenderedElementState::Indexed {
            node: ElementNodeId(2),
            cells: vec![2, 50],
        },
        RenderedElementState::Fixture {
            node: ElementNodeId(3),
            color,
            state: state.clone(),
        },
    ];
    let patch = PreparedPatch {
        steps: vec![
            source(0, 0, 2),
            PatchStep::Filter {
                input: 0,
                output_start: 1,
                filter: discrete.clone(),
            },
            PatchStep::Filter {
                input: 1,
                output_start: 2,
                filter: prepare_filter(&FilterDefinition::Quantize8 { width: 4 }).unwrap(),
            },
            PatchStep::Sink {
                input: 2,
                frame: 0,
                start: 0,
                end: 4,
            },
            source(3, 1, 2),
            PatchStep::Filter {
                input: 3,
                output_start: 4,
                filter: indexed.clone(),
            },
            PatchStep::Filter {
                input: 4,
                output_start: 5,
                filter: prepare_filter(&FilterDefinition::Quantize8 { width: 2 }).unwrap(),
            },
            PatchStep::Sink {
                input: 5,
                frame: 0,
                start: 4,
                end: 6,
            },
            source(6, 2, 1),
            PatchStep::Fixture {
                input: 6,
                output_start: 7,
                program: 0,
            },
            PatchStep::Sink {
                input: 7,
                frame: 0,
                start: 6,
                end: 14,
            },
        ]
        .into_boxed_slice(),
        value_layouts: Box::new([
            PatchValueLayout::Color(2),
            PatchValueLayout::Components(4),
            PatchValueLayout::Slots(4),
            PatchValueLayout::Indexed(2),
            PatchValueLayout::Components(2),
            PatchValueLayout::Slots(2),
            PatchValueLayout::Fixture {
                width: 1,
                functions: 3,
            },
            PatchValueLayout::Slots(8),
        ]),
        fixture_programs: Box::new([program]),
    };
    let mut workspace = patch.workspace();
    let mut output = [[255u8; 16]];
    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let result = (0..100).try_for_each(|_| patch.evaluate(&elements, &mut output, &mut workspace));
    COUNTING.store(false, Ordering::Relaxed);
    result.unwrap();
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), 0);
    assert_eq!(
        output[0],
        [0, 64, 0, 64, 51, 204, 0, 64, 150, 96, 32, 0, 32, 0, 0, 0]
    );

    let mut component_output = [PatchValue::new(PatchValueLayout::Components(4))];
    let unknown = Color {
        red: color.red + 1,
        ..color
    };
    assert_eq!(
        discrete.evaluate(&PatchValue::Colors(vec![unknown; 2]), &mut component_output),
        Err(FilterError::UnsupportedDiscreteColor(unknown))
    );
    assert_eq!(
        indexed.evaluate(&PatchValue::Indexed(vec![2, 99]), &mut component_output),
        Err(FilterError::MissingIndexedMapping(99))
    );
    let program = &patch.fixture_programs[0];
    let mut fixture_output = [255; 16];
    program
        .encode(&[state.clone(), state.clone()], &mut fixture_output)
        .unwrap();
    assert_eq!(&fixture_output[..8], &output[0][6..14]);
    assert_eq!(&fixture_output[8..], &output[0][6..14]);
    assert_eq!(
        program.encode(std::slice::from_ref(&state), &mut fixture_output),
        Err(FixtureEncodingError::WidthMismatch)
    );

    profile.functions.get_mut(&mixing).unwrap().kind = FixtureFunctionKind::ColorMixing {
        model: ColorMixingModel::Rgb,
    };
    profile.channels.retain(|channel| channel.slot != 6);
    profile.validate().unwrap();
    let rgb_program = prepare_fixture_encoding(&profile, 8).unwrap();
    let mut rgb_output = [255; 8];
    rgb_program.encode(&[state], &mut rgb_output).unwrap();
    assert_eq!(&rgb_output[3..6], &[128, 64, 32]);
}
