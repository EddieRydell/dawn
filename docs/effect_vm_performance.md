# Effect VM Performance

Benchmarks were run on May 31, 2026 on the local Windows development machine.
All comparable rows use the release build and the club-rig benchmark at `time=42`:

```powershell
cargo run --release -p dawn-cli -- bench-effect examples/club-rig --time 42 --iterations 300 --warmup 30 --no-effect-breakdown
```

This benchmark has one active `effects.MarkPulse` effect targeting 200 pixels.

## Comparable Release Results

| Level | Commit | Whole frame p50 | Effect p50 | Per sample p50 | Whole frame speedup |
| --- | --- | ---: | ---: | ---: | ---: |
| Interpreted | `571721b` | 3.400 ms | 3.320 ms | 0.017 ms | 1.00x |
| Initial bytecode | `0ac1f9e` | 0.749 ms | 0.725 ms | 0.004 ms | 4.54x |
| First optimization | `af5444f` | 0.278 ms | 0.250 ms | 0.001 ms | 12.23x |
| Evaluator cache + builtin opcodes | local working tree | 0.236 ms | 0.236 ms | 0.001 ms | 14.41x |
| Typed destination-slot VM | local working tree | 0.126 ms | n/a | n/a | 26.98x |

## Step Changes

| Change | Whole frame p50 improvement | Effect p50 improvement |
| --- | ---: | ---: |
| Interpreted to initial bytecode | 4.54x | 4.58x |
| Initial bytecode to first optimization | 2.69x | 2.90x |
| First optimization to evaluator cache + builtin opcodes | 1.18x | 1.06x |
| Evaluator cache + builtin opcodes to typed destination-slot VM | 1.87x | n/a |
| Interpreted to typed destination-slot VM | 26.98x | n/a |

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
| Typed destination-slot VM | 1,000 | 200,000 | 105,000 instructions, 22,000 constants, 9,000 param slots, 93,000 typed register slots | 132.489 ms | n/a |

The second pass improves the synthetic whole-frame p50 by 1.01x and the displayed first-effect
p50 by 1.06x against the first optimization row. The synthetic whole-frame result is intentionally
close because the benchmark still executes 1,000 independent VM render paths.

The typed VM result uses `--no-effect-breakdown`, which isolates whole-frame rendering from
per-effect report generation. In that mode the synthetic whole-frame p50 improves by 1.94x against
the evaluator cache + builtin opcode row.

The non-isolated report path was also checked after the typed VM change:

```powershell
cargo run --release -p dawn-cli -- bench-effect examples/club-rig --time 42 --iterations 30 --warmup 5
```

It reported whole-frame p50 0.128 ms, effect p50 0.124 ms, and per-sample p50 0.001 ms.

## Typed VM Profile

The post-change synthetic benchmark was profiled with:

```powershell
samply record --save-only --output target\effect-vm-typed-synthetic-profile.json.gz -- target\release\dawn.exe bench-effect examples\club-rig --time 42 --iterations 100 --warmup 10 --synthetic-active-effects 1000 --no-effect-breakdown
```

The local Windows capture contained samples but did not symbolize Rust frames, even when rebuilt
with debug info and recorded with `--symbol-dir target\release`. The profile string table no longer
contains the old stack VM markers `VmValue`, `LoadLocal`, `StoreLocal`, or `stack underflow`; those
mechanics were removed from the bytecode and runtime implementation.

## Notes

- The first optimization keeps the public runtime API intact while changing VM internals to borrowed stack/local values and typed bytecode operations.
- The optimized path also prepares effect params directly from sequence param documents in render hot paths.
- Mark helper lookups now use binary search over sorted marks.
- The second optimization pass adds a reusable sequence frame evaluator, caches fixture templates and prepared effect render data, and emits dedicated VM opcodes for hot builtins instead of generic builtin calls.
- The typed VM pass replaces value-stack bytecode with typed destination slots and reuses typed scratch arrays per prepared effect during sequence frame evaluation.
- `bench-effect --no-effect-breakdown` reports whole-frame timing only, while still reporting active effect count, target pixel samples per frame, and aggregate bytecode/register stats.
- Timings are observational only. There are no hard performance assertions because machine load and build state can move these numbers.
