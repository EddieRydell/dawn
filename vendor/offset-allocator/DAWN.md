# Dawn's vendored offset-allocator

Source: the crates.io `offset-allocator` 0.2.0 release, downloaded with
`cargo info offset-allocator@0.2.0`. Upstream:
<https://github.com/pcwalton/offset-allocator/>.
The release archive SHA-256 is
`e234d535da3521eb95106f40f0b73483d80bfb3aacf27c40d7e2b72f1a3e00a2`.
The upstream MIT license, README, implementation and tests are retained.
`Cargo.toml` is based on the release's `Cargo.toml.orig`.

Local changes:

- Enable `no_std`, import `alloc` vectors and `core` formatting, and use
  `core::array` in the upstream tests.
- Disable `nonmax`'s default `std` feature.
- Remove debug logging and the `log` dependency. An application logger must not
  introduce hidden heap allocations into allocation/free operations. No global
  log feature flags are changed.

The allocation algorithm and public API are unchanged. The crate is excluded
from Dawn's workspace membership so workspace formatting/lints do not rewrite
third-party code. Test it separately with
`cargo test --manifest-path vendor/offset-allocator/Cargo.toml`.
Dawn's regular runtime test suite also checks range reuse, exhaustion,
coalescing, and zero system-heap allocations after construction.

## Integration constraints

- Use `Allocator<u32>::with_max_allocs`, not `new`: the latter reserves 131,072
  metadata nodes by default. Node capacity is not the number of live arrays;
  free fragments and allocator bookkeeping also consume nodes.
- Construction and `reset()` allocate. Reuse ranges through `free()` during
  evaluation; do not call `reset()` in the hot path.
- This manages offsets only. Dawn still needs a separately prepared value
  buffer, array ownership/reclamation, and a capacity bound that accounts for
  fragmentation and bin rounding. It does not itself eliminate `MakeArray`'s
  allocations.
- The `u16` variant also narrows returned offsets, not just metadata indices.
  Use `u32` for Dawn; do not silently truncate a larger value buffer.
- Validate capacities before construction (in particular, node count must be
  at least two and below the upstream maximum). Allocation can return `None`
  with free space still available due to fragmentation, bin rounding, or node
  exhaustion; total live element count alone is not a sufficient bound.
- The fixed 256-bin table and per-node metadata are real RAM overhead. Measure
  them alongside the value buffer before choosing per-workspace capacities.

This is a local maintained port, not an upstream no_std release or a claim of
ESP32 board-level validation.
