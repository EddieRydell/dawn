use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use offset_allocator::Allocator;

struct CountingAllocator;

thread_local! {
    // Count only allocations on the test thread, inside an explicit measurement window.
    static COUNTS: Cell<Option<(usize, isize)>> = const { Cell::new(None) };
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let _ = COUNTS.try_with(|counts| {
                if let Some((calls, bytes)) = counts.get() {
                    counts.set(Some((calls + 1, bytes + layout.size() as isize)));
                }
            });
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let _ = COUNTS.try_with(|counts| {
            if let Some((calls, bytes)) = counts.get() {
                counts.set(Some((calls, bytes - layout.size() as isize)));
            }
        });
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[test]
fn bounded_offset_storage_reuses_ranges_without_heap_allocations() {
    COUNTS.set(Some((0, 0)));
    let mut allocator = Allocator::<u32>::with_max_allocs(256, 32);
    let (setup_calls, metadata_bytes) = COUNTS.replace(None).unwrap();
    assert_eq!(setup_calls, 2);
    // These are allocator metadata only: the VM's value buffer is separate.
    eprintln!(
        "offset allocator: {} inline bytes + {metadata_bytes} heap bytes for 32 nodes",
        size_of::<Allocator<u32>>()
    );

    let mut live = [None; 8];
    COUNTS.set(Some((0, 0)));
    for i in 0..10_000 {
        let slot = i % live.len();
        if let Some(old) = live[slot].take() {
            allocator.free(old);
        }
        let size = (i % 17 + 1) as u32;
        let allocation = allocator.allocate(size).expect("bounded live working set");
        assert_eq!(allocator.allocation_size(allocation), size);
        let end = allocation.offset + size;
        assert!(end <= 256);
        for other in live.iter().flatten() {
            assert!(
                end <= other.offset
                    || other.offset + allocator.allocation_size(*other) <= allocation.offset
            );
        }
        live[slot] = Some(allocation);
    }
    for allocation in live.into_iter().flatten() {
        allocator.free(allocation);
    }
    // Reclamation must coalesce the whole region without calling reset().
    let whole = allocator.allocate(256).expect("all ranges reclaimed");
    assert_eq!(whole.offset, 0);
    assert!(allocator.allocate(1).is_none());
    allocator.free(whole);
    assert_eq!(allocator.storage_report().total_free_space, 256);
    let counts = COUNTS.replace(None).unwrap();
    assert_eq!(
        counts,
        (0, 0),
        "allocate/free must not touch the system heap"
    );

    // Metadata exhaustion must also return None rather than growing a table.
    let mut allocator = Allocator::<u32>::with_max_allocs(256, 4);
    COUNTS.set(Some((0, 0)));
    let first = allocator.allocate(1).unwrap();
    let second = allocator.allocate(1).unwrap();
    assert!(allocator.allocate(1).is_none());
    allocator.free(first);
    allocator.free(second);
    let whole = allocator.allocate(256).unwrap();
    allocator.free(whole);
    assert_eq!(COUNTS.replace(None).unwrap(), (0, 0));
}
