use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use dawn_language::dsl::{BoundParams, Identifier, ParamDecl, Type, Value};
use dawn_language::sequence::{AutomationClip, AutomationClipId, AutomationMapping};
use dawn_language::values::{Curve, CurvePoint, DawnDuration, DawnTime};
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

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
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

    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let result = samples
        .into_iter()
        .try_for_each(|seconds| automated.apply_automation(0, &clip.curve, &mapping, seconds));
    COUNTING.store(false, Ordering::Relaxed);

    result.expect("measured automation should apply");
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), 0);

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
    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let result = samples
        .into_iter()
        .try_for_each(|position| bound.apply_automation(0, &clip.curve, &mapping, position));
    COUNTING.store(false, Ordering::Relaxed);
    result.unwrap();
    assert_eq!(bound.enum_name(0).unwrap(), options[0].as_str());
    assert_eq!(
        ALLOCATIONS.load(Ordering::Relaxed),
        0,
        "enum automation allocated"
    );

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
    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let sampled = effect.sample_bound(&params, &context, &mut workspace);
    COUNTING.store(false, Ordering::Relaxed);
    assert_eq!(sampled.unwrap(), expected);
    assert_eq!(
        ALLOCATIONS.load(Ordering::Relaxed),
        0,
        "constant arrays allocated"
    );
}
