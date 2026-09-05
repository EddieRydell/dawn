use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use dawn_language::dsl::{BoundParams, Identifier, ParamDecl, Type, Value};
use dawn_language::sequence::{AutomationClip, AutomationClipId, AutomationMapping};
use dawn_language::values::{Curve, CurvePoint, DawnDuration, DawnTime};
use indexmap::IndexMap;

struct CountingAllocator;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static LIVE_BYTES: Cell<isize> = const { Cell::new(0) };
    static PEAK_BYTES: Cell<usize> = const { Cell::new(0) };
}

fn record_memory_change(allocated: usize, freed: usize) {
    // Ignore allocations on other test threads and during thread-local teardown.
    if COUNTING.try_with(Cell::get).unwrap_or(false) {
        if allocated != 0 {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        }
        let _ = LIVE_BYTES.try_with(|live| {
            let bytes = live.get() + allocated as isize - freed as isize;
            live.set(bytes);
            let _ = PEAK_BYTES.try_with(|peak| peak.set(peak.get().max(bytes.max(0) as usize)));
        });
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record_memory_change(layout.size(), 0);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        record_memory_change(0, layout.size());
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_memory_change(layout.size(), 0);
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, size) };
        if !pointer.is_null() {
            record_memory_change(size, layout.size());
        }
        pointer
    }
}

#[test]
fn warmed_curve_enum_automation_and_constant_arrays_do_not_allocate() {
    let declarations = [ParamDecl {
        name: Identifier::new("shape".to_string()).expect("valid identifier"),
        ty: Type::Curve,
        default: Some(Value::Curve(Arc::new(Curve { points: Vec::new() }))),
    }];
    let base = BoundParams::bind(&declarations, &IndexMap::new()).expect("curve should bind");
    let mut automated = base.clone();
    let clip = AutomationClip {
        id: AutomationClipId(1),
        start: DawnTime::from_micros(0),
        duration: DawnDuration::from_micros(1_000_000),
        anchor_lane_index: 0,
        lane_index: 0,
        curve: Curve {
            points: vec![
                CurvePoint {
                    position: 0.0,
                    value: 0.0,
                },
                CurvePoint {
                    position: 0.25,
                    value: 1.0,
                },
                CurvePoint {
                    position: 0.5,
                    value: 0.25,
                },
                CurvePoint {
                    position: 0.75,
                    value: 0.75,
                },
                CurvePoint {
                    position: 1.0,
                    value: 0.0,
                },
            ],
        },
        bindings: Vec::new(),
        detached_bindings: Vec::new(),
    };
    let mapping = AutomationMapping::Curve {
        min: -1.0,
        max: 2.0,
    };
    let samples = [0.0, 0.25, 0.5, 0.75, 1.0];
    for seconds in samples {
        automated
            .apply_automation(0, &clip.curve, &mapping, seconds)
            .expect("warmup automation should apply");
    }

    ALLOCATIONS.set(0);
    COUNTING.set(true);
    let result = samples
        .into_iter()
        .try_for_each(|seconds| automated.apply_automation(0, &clip.curve, &mapping, seconds));
    COUNTING.set(false);

    result.expect("measured automation should apply");
    assert_eq!(ALLOCATIONS.get(), 0);

    let options =
        ["short", "much_longer_option"].map(|value| Identifier::new(value.into()).unwrap());
    let declarations = [ParamDecl {
        name: Identifier::new("mode".into()).unwrap(),
        ty: Type::Enum(options.to_vec()),
        default: Some(Value::Enum(options[0].clone())),
    }];
    let mut bound = BoundParams::bind(&declarations, &IndexMap::new()).unwrap();
    let mapping = AutomationMapping::Enum {
        values: options.to_vec(),
    };
    // Visit the longest option first so subsequent updates must reuse its storage.
    bound
        .apply_automation(0, &clip.curve, &mapping, 0.25)
        .unwrap();
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    let result = samples
        .into_iter()
        .try_for_each(|position| bound.apply_automation(0, &clip.curve, &mapping, position));
    COUNTING.set(false);
    result.unwrap();
    assert_eq!(bound.enum_name(0).unwrap(), options[0].as_str());
    assert_eq!(ALLOCATIONS.get(), 0, "enum automation allocated");

    let effect = dawn_language::dsl::compile_effects(
        "effect Constants { color sample() {
            array<array<float>> values = [[0.1, 0.2], [0.3, 0.4]];
            return rgb(values[0][1], values[1][0], values[1][1]);
        } }",
    )
    .unwrap()
    .remove(0);
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let mut workspace = dawn_language::dsl::VmWorkspace::default();
    let context = dawn_language::dsl::RunContext {
        progress: 0.0,
        time: dawn_language::values::SampleDuration::from_ticks(0),
        duration: dawn_language::values::SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    let expected = effect
        .sample_bound(&params, &context, &mut workspace)
        .unwrap();
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    let sampled = effect.sample_bound(&params, &context, &mut workspace);
    COUNTING.set(false);
    assert_eq!(sampled.unwrap(), expected);
    assert_eq!(ALLOCATIONS.get(), 0, "constant arrays allocated");
}

#[test]
#[ignore = "MakeArray still allocates; enable when calculated-array storage is preallocated"]
fn calculated_arrays_do_not_allocate_after_warmup() {
    let effect =
        dawn_language::dsl::compile_effects(include_str!("fixtures/array-lifetimes.effect.dawn"))
            .unwrap()
            .remove(0);
    let mut workspace = dawn_language::dsl::VmWorkspace::default();
    let mut counts = [0; 3];
    let mut peaks = [0; 3];
    for (case, iterations) in [2, 64, 9_999].into_iter().enumerate() {
        let params = effect
            .bind_params(&IndexMap::from([(
                Identifier::new("iterations".into()).unwrap(),
                Value::Int(iterations),
            )]))
            .unwrap();
        let context = dawn_language::dsl::RunContext {
            progress: 0.25,
            time: dawn_language::values::SampleDuration::from_ticks(0),
            duration: dawn_language::values::SampleDuration::from_ticks(1_000_000),
            pixel_index: 0,
            pixel_count: 1,
            pixel_fraction: 0.0,
        };
        effect
            .sample_bound(&params, &context, &mut workspace)
            .unwrap();
        ALLOCATIONS.set(0);
        // This fixture releases every calculated array before sample_bound returns.
        // Count only new allocation payloads, excluding the already-warmed workspace.
        LIVE_BYTES.set(0);
        PEAK_BYTES.set(0);
        COUNTING.set(true);
        let results = [0.25, 0.5].map(|progress| {
            effect.sample_bound(
                &params,
                &dawn_language::dsl::RunContext {
                    progress,
                    ..context.clone()
                },
                &mut workspace,
            )
        });
        COUNTING.set(false);
        counts[case] = ALLOCATIONS.get();
        peaks[case] = PEAK_BYTES.get();
        assert_eq!(
            LIVE_BYTES.get(),
            0,
            "sample retained newly allocated storage"
        );
        assert_eq!(
            results.map(Result::unwrap),
            [
                dawn_language::dsl::Color {
                    red: 64,
                    green: 89,
                    blue: 230
                },
                dawn_language::dsl::Color {
                    red: 128,
                    green: 153,
                    blue: 230
                },
            ]
        );
    }
    assert_eq!(
        counts,
        [0, 0, 0],
        "allocation calls for two samples at 2, 64, and 9999 loop iterations; peak newly allocated payload bytes: {peaks:?}"
    );
}
