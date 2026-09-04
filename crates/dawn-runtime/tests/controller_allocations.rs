use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use camino::Utf8PathBuf;
use dawn_language::dsl::Identifier;
use dawn_language::sequence::AutomationTarget;
use dawn_language::values::sample_time_from_frame;
use dawn_project_io::load_package;
use dawn_runtime::{PreparedSequenceOutput, SequenceOutputScratch};

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
fn warmed_controller_sampling_does_not_allocate() {
    let project_path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/starter");
    let session = load_package(&project_path)
        .expect("starter project should load")
        .session;
    for sequence_id in &session.project.root.sequences {
        let output = PreparedSequenceOutput::prepare(
            &session.project,
            &session.project.root.setup,
            sequence_id,
        )
        .expect("starter output should prepare");
        let frames = [
            0,
            output.frame_count() / 2,
            output.frame_count().saturating_sub(1),
        ];
        assert_warmed_sampling_does_not_allocate(&output, &frames, sequence_id.0.object());
    }

    let mut project = session.project;
    let sequence_id = project
        .root
        .sequences
        .iter()
        .find(|id| id.0.object() == "layer_test")
        .expect("starter project should include layer_test")
        .clone();
    let sequence = project
        .sequences
        .get_mut(&sequence_id)
        .expect("layer_test should resolve");
    sequence.automation_clips[0].bindings[0].target = AutomationTarget::EffectParam {
        effect_id: sequence.effects[0].id.clone(),
        param: Identifier::new("pulse_overlap".to_string()).expect("static identifier is valid"),
    };
    let output = PreparedSequenceOutput::prepare(&project, &project.root.setup, &sequence_id)
        .expect("automated native output should prepare");
    assert_warmed_sampling_does_not_allocate(
        &output,
        &[7150, 7151, 7152],
        "automated native effect",
    );
}

fn assert_warmed_sampling_does_not_allocate(
    output: &PreparedSequenceOutput,
    frames: &[u32],
    name: &str,
) {
    let mut scratch = SequenceOutputScratch::default();
    for &frame in frames {
        let time = sample_time_from_frame(frame, output.frame_rate())
            .expect("sample frame should fit the controller clock");
        output
            .sample_into(time, &mut scratch)
            .expect("warmup sample should render");
    }

    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let result = frames.iter().try_for_each(|&frame| {
        let time = sample_time_from_frame(frame, output.frame_rate())
            .expect("sample frame should fit the controller clock");
        output.sample_into(time, &mut scratch).map(|_| ())
    });
    COUNTING.store(false, Ordering::Relaxed);

    result.expect("measured samples should render");
    assert_eq!(
        ALLOCATIONS.load(Ordering::Relaxed),
        0,
        "warmed sequence {name} allocated"
    );
}
