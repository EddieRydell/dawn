# Effect VM Performance

Benchmarks were run on May 31, 2026 on the local Windows development machine.
All comparable rows use the release build and the club-rig benchmark at `time=42`:

```powershell
cargo run --release -p dawn-cli -- bench-effect examples/club-rig --time 42 --iterations 300 --warmup 30
```

This benchmark has one active `effects.MarkPulse` effect targeting 200 pixels.

## Comparable Release Results

| Level | Commit | Whole frame p50 | Effect p50 | Per sample p50 | Whole frame speedup |
| --- | --- | ---: | ---: | ---: | ---: |
| Interpreted | `571721b` | 3.400 ms | 3.320 ms | 0.017 ms | 1.00x |
| Initial bytecode | `0ac1f9e` | 0.749 ms | 0.725 ms | 0.004 ms | 4.54x |
| First optimization | `af5444f` | 0.278 ms | 0.250 ms | 0.001 ms | 12.23x |
| Evaluator cache + builtin opcodes | local working tree | 0.236 ms | 0.236 ms | 0.001 ms | 14.41x |

## Step Changes

| Change | Whole frame p50 improvement | Effect p50 improvement |
| --- | ---: | ---: |
| Interpreted to initial bytecode | 4.54x | 4.58x |
| Initial bytecode to first optimization | 2.69x | 2.90x |
| First optimization to evaluator cache + builtin opcodes | 1.18x | 1.06x |
| Interpreted to evaluator cache + builtin opcodes | 14.41x | 14.07x |

## First Optimization Synthetic Load

The synthetic active-effect benchmark was added in the first optimization commit, so the older
commits do not expose the same command-line flag. The current high-load coverage was measured with:

```powershell
cargo run --release -p dawn-cli -- bench-effect examples/club-rig --time 42 --iterations 100 --warmup 10 --synthetic-active-effects 1000
```

| Level | Active effects | Target pixel samples/frame | Aggregate bytecode | Whole frame p50 | Effect p50 |
| --- | ---: | ---: | --- | ---: | ---: |
| First optimization | 1,000 | 200,000 | 132,000 instructions, 22,000 constants, 9,000 param slots, 16,000 local slots, max stack 4 | 258.242 ms | 0.247 ms |
| Evaluator cache + builtin opcodes | 1,000 | 200,000 | 132,000 instructions, 22,000 constants, 9,000 param slots, 16,000 local slots, max stack 4 | 256.900 ms | 0.232 ms |

The second pass improves the synthetic whole-frame p50 by 1.01x and the displayed first-effect
p50 by 1.06x against the first optimization row. The synthetic whole-frame result is intentionally
close because the benchmark still executes 1,000 independent VM render paths.

## Notes

- The first optimization keeps the public runtime API intact while changing VM internals to borrowed stack/local values and typed bytecode operations.
- The optimized path also prepares effect params directly from sequence param documents in render hot paths.
- Mark helper lookups now use binary search over sorted marks.
- The second optimization pass adds a reusable sequence frame evaluator, caches fixture templates and prepared effect render data, and emits dedicated VM opcodes for hot builtins instead of generic builtin calls.
- Timings are observational only. There are no hard performance assertions because machine load and build state can move these numbers.
