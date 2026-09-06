# Runtime optimization results

> Historical record. These results predate the final execution audit; use
> [the execution audit](execution_audit_2026-09-06.md) for current claims.

Status: complete. Normal ESP32 validation, full desktop comparison, final PC
profiling and repository checks passed. Validated normal firmware is restored.
This report distinguishes measured results from remaining work.

## ESP32 runtime

Classic ESP32, one 240 MHz core, Wi-Fi off, no LEDs or physical output. These are
compute-only times for evaluating and patching a prepared show, not wire-rate FPS.
Baseline: `firmware/esp32/results/2026-09-04-array-storage.txt`.
Current: `firmware/esp32/results/2026-09-05-normal-uniform-loop.txt`.
Current ELF SHA256:
`e0654c710d0f3f4b6507d5a05b0ccfcba70de214efc02bdca38b7ccf239ce07a`.

Both use the same original 160 cases. The current capture adds eight exact-output
uniform-reuse controls, for 168 total. Each measures a complete 32-frame cycle.

| 200-pixel workload | Before, us/frame | After, us/frame | Reduction | Compute FPS |
| --- | ---: | ---: | ---: | ---: |
| Uniform fade | 208 | 102 | 51.0% | 9,804 |
| Pixel ramp | 742 | 640 | 13.7% | 1,562 |
| Ramp through an operator, prefix reuse | 2,201 | 1,607 | 27.0% | 622 |
| Two-level nested fixture | 6,124 | 2,960 | 51.7% | 338 |
| Eight-level nested fixture | 15,478 | 11,073 | 28.5% | 90 |
| Sixteen uniform layers | 1,998 | 1,525 | 23.7% | 656 |
| ScanSweep | 5,249 | 5,146 | 2.0% | 194 |
| ImpactBurst | 5,449 | 5,346 | 1.9% | 187 |
| SparkleComet | 8,314 | 8,212 | 1.2% | 122 |
| ShimmerField | 5,239 | 5,136 | 2.0% | 195 |

FPS is approximate, calculated from the integer-microsecond reported mean.
At 1,600 pixels, uniform fade is 1,564 -> 723 us, two-level nesting is
48,899 -> 23,502 us, and sixteen uniform layers are 14,916 -> 11,247 us.
The raw-VM control cases are essentially unchanged: their largest increase is
one microsecond, 0.03%. Gains principally come from prepared execution and routing,
not a claim that every interpreter instruction became faster.

All 168 cases passed their timed-frame golden checksums. Successful prepared first
frames and warmed frames made zero heap allocations. All allocations were recovered
at capture end (`heap_free=163840`). This does not mean the runtime is heapless:
workspace creation owns reserved vectors and bounded array storage. Errors can
allocate diagnostic strings; raw unprepared VM first calls can allocate registers.

ScanSweep retained memory fell from 12,124 to 9,860 bytes at 200 pixels, and
75,124 to 57,460 bytes at 1,600. Dense source routing now stores contiguous spans,
and runtime output borrows the graph's existing buffer instead of copying into a
second frame buffer. Fragmented source routing can cost more metadata per cell
(12-byte singleton span versus the old 8-byte address); it preserves exact order
and repeated cells rather than assuming all patches are contiguous.

## Desktop

Full Criterion comparison completed with 100 samples, 3-second warmup and
5-second measurement windows. Values below consistently use
`estimates.json.mean.point_estimate`, not a mixture of means and console slopes.

| Workload | Before, us | After, us | Reduction |
| --- | ---: | ---: | ---: |
| Raw DSL suite, four effects × 512 pixels | 468.700 | 436.995 | 6.8% |
| Representative frames | 3,242.388 | 2,965.799 | 8.5% |
| Dense 60-frame playback | 8,168.765 | 7,640.851 | 6.5% |
| Dense 60-frame controller output | 8,628.220 | 7,843.664 | 9.1% |
| Chase/pulse, one layer | 6.368 | 4.958 | 22.1% |
| Chase/pulse, four layers | 14.346 | 11.306 | 21.2% |
| Chase/pulse, sixteen layers | 56.374 | 45.724 | 18.9% |
| Device-sized mark pulse | 14.186 | 10.504 | 26.0% |
| Device-sized mark chase | 235.973 | 188.912 | 19.9% |
| Larger mark pulse | 556.201 | 445.137 | 20.0% |
| Larger mark chase | 4,480.590 | 3,830.471 | 14.5% |
| Sixteen uniform layers | 12.009 | 8.637 | 28.1% |
| Sixteen pixel-ramp layers | 88.380 | 80.944 | 8.4% |

All 56 current benchmarks have lower measured means than their saved tags in the
final run. This is not a universal speed guarantee; some small differences are
within Criterion's configured threshold. Retired benchmark folders remain in
`target/criterion` and are excluded from this comparison. Intermediate pixel/array
layer regressions were confirmed using an unchanged executable, then resolved by
separating the existing layer loop and removing its per-pixel uniform-cache branch.
The new edge-fade and uniform-resource/upstream control
baselines were established after their implementations and are not presented as
pre-goal comparisons. The final host executable is preserved at
`target/final-render-bench.exe`, SHA256
`8f875b5238cec0c7f61e01bee8e267ce112ea76b6d05bc332de6cd756f6dfab2`.

## Profiling

The optional firmware profiler samples interrupted PCs into bounded static storage.
The host resolves linker symbols and DWARF leaf locations against the exact ELF.
It does not reconstruct dynamic call stacks or produce a true stack flame graph.
No profiling instrumentation was added to the portable runtime.

Accepted 15-fixture, 60-window evidence:
`firmware/esp32/results/2026-09-05-pc-final.txt`, ELF SHA256
`43b04ae797d3c335ed62400121733861f09007320c86f710f233e5dccff3f23d`.
Each fixture runs disabled / 997-us sampling / 1999-us sampling / disabled,
finishing whole 32-frame cycles. Sampling overhead is 0.355–0.408% and
0.176–0.211%, respectively. Disabled controls differ by at most 0.002% within
this image. Periodic aliasing, interrupt masking, unresolved ROM locations, and
cross-image code placement remain limitations.

The PC harness checks heap-call counts over each complete window and compares its
last timed frame to the host golden outside timing. It does not checksum every
timed frame; the separate normal harness does that for its 168 cases.

In that profiling image, 200-pixel one/four/sixteen stacked chase-pulse fixtures
take 1.218 / 2.652 / 10.534 ms per frame. MarkPulse takes 2.206 ms, MarkChase
48.144 ms, and nonzero-edge-fade MarkPulse 2.499 ms. These are distinct workloads,
not interchangeable definitions of a layer. They are not substituted for the normal
image's timings. Native chase arithmetic, including software float division,
remains expensive; complex DSL effects spend much of their time in VM dispatch,
register operations, and arithmetic. Zero heap calls do not eliminate bounded
array allocator bookkeeping or reference cleanup. ScanSweep's VM execution accounts
for 61–64% of samples across the two rates, and software float division 11–14%.
The one-layer chase/pulse fixture spends about 41% in software float division at
1999-us sampling; MarkChase spends about 41% there at 997-us sampling. These are
sampled leaf percentages, not inclusive call-stack percentages.

UART captures sometimes fail with non-ASCII 0xff bursts; flashing has also produced
checksum errors or timeouts. Their cause is unproven. Failed captures are rejected
entirely, never repaired by dropping bytes. A separate earlier one-expression
integer/float comparison used `pc-edge-integer-retry.txt` (integer ELF
`20064242af237a6d10a23c63fa89e8df027e76661b80657baf9b932256f4c4eb`).
Its matching float-control capture passed all 60 windows in
`pc-edge-float-control-repeat3.txt`, using ELF
`9c9102c89891cdeba2d7cfe3580648a39cd874dc5c654e721a9506c704f17944`.
Nonzero-fade MarkPulse takes 2.846 ms with float indexing versus 2.535 ms with
integer indexing, a 10.9% reduction. The otherwise unchanged zero-fade fixture is
0.32% slower in the integer image; MarkChase is 0.08% slower, while ScanSweep is
essentially unchanged. These smaller image-layout differences are recorded rather
than assumed nonexistent. Expanded native/DSL parity tests cover nonzero fade.

## Simplicity and regressions

The main changes reuse upstream timing/automation/uniform computation, hoist safe
uniform scalar/resource work in compilation, pack actual automation workspaces,
borrow graph output, and elaborate source routing into spans. No separate embedded
runtime or new effect categories were introduced. `dawn-runtime` remains `no_std`;
the firmware uses its non-atomic feature configuration and the same implementation.

Regressions led to concrete changes:

- Borrowing graph output initially enlarged the inlined output function and slowed
  simple paths. Keeping the existing graph evaluator as a separate IRAM function
  recovered performance without restoring the redundant frame buffer.
- Moving complete-result caching into the VM added duplicate validity state and
  returned after parameter/context setup. Repeated laptop and board measurements
  confirmed the slowdown: uniform reuse was 988/7,787 us at 200/1,600 pixels versus
  862/6,781 us in the earlier graph-cache image. The duplicate VM cache was removed;
  successful sample identity and color now stay together at the earlier graph return.
- Simple desktop pixel layers regressed inside the large graph evaluator.
  Outlining the existing layer loop recovered their performance, but initially
  added about 7% to very small uniform ESP32 cases. Sampling uniform effects once
  before their composition loop removed that per-pixel branch and recovered more
  than the loss. No effect-specific executor or new cache was added. The final
  controller desktop mean is about 2% above the best intermediate consolidated
  image, while remaining 9% below goal start; the different function boundaries
  trade some whole-project throughput for simpler, faster small-effect loops.
  Exact hardware stall/register-pressure attribution is not claimed.

Source count is 17,292 nonblank Rust lines in 55 files, versus 16,653 in 54 at the
goal's start: **+639 lines**, not a net reduction. This scope includes runtime/src,
language/src/dsl, elaboration/src, and firmware/src, including their inline tests;
it excludes integration-test directories, benchmarks, Python, and documentation.
Profiling and expanded shared workloads contribute to growth. The failed VM-cache
experiment alone was reduced by 74 lines when its duplicate state and test were
removed. Detailed experiment history is in `runtime_optimization_2026-09-05.md`.

## Validation

- Passed full `pnpm check`, firmware release build and both-bin clippy.
- Passed all 11 uniform-sampling tests, six PC collector contract tests, and firmware formatting.
- Passed final 168-case board capture and complete 56-case desktop comparison.
- Passed final PC capture: all 60 windows, bounded sample storage and allocation checks.
- Restored the validated normal harness to COM4; final ELF hashes and capture counts rechecked.

The implementation evidence is located as follows.

| Requirement | Implementation and verification evidence |
| --- | --- |
| Shared stateless embedded runtime | `dawn-runtime/src/lib.rs` is `no_std`; firmware depends on it with default features disabled; real Xtensa builds and captures exercise it. |
| Upstream timing, automation, uniform reuse | `evaluation.rs` caches successful effect/node time identities; `prepared_uniform.rs` compares full execution across multiple effects, nested siblings, temporal revisits, automation edits and backward seeks. |
| Safe compilation/elaboration work | `dsl/optimize.rs::hoist_uniform` preserves ordered resource barriers; resource branch/error tests and native/DSL integer-section parity tests protect semantics. |
| Prepared graph and output work | Elaboration prepares frame slots and source spans; runtime borrows graph output. Graph alias tests, `prepared_patch.rs` order/bounds/allocation checks, and `show_workspace.rs` fresh/reused seek comparisons cover these changes. |
| Bounded prepared memory | Workspace construction reserves VM/array/automation/patch storage. Controller allocation tests and normal firmware count first-frame and warmed heap calls. |
| Relevant performance coverage | Shared fixtures cover complex DSL effects, layered simple effects, operators, temporal sampling, chase/pulse stacks and marks; desktop Criterion and exact-ELF ESP32 records are retained. |
| Attribution profiling | Optional firmware-only PC sampler, two periods and disabled controls, exact-ELF symbols/DWARF, strict collector contract tests and accepted 60-window runs. |
| Regression investigation | Repeated same-executable desktop runs, exact-image board controls, cache-ownership experiment, and outlined graph/layer experiments are recorded chronologically. |
| Checks and reporting | Repository `pnpm check`, standalone firmware build/clippy and collector tests; measured timing, memory, source count, accuracy and limitations in this report and the experiment log. |

## Audit boundaries

The requirement review found two limits that must not be hidden by the passing
tests. Prepared structures are trusted elaboration output: public fields can be
mutated into invalid indices or capacity declarations, and the runtime is not an
untrusted-bytecode verifier. Workspace creation reserves against that prepared
structure; callers must not mutate it underneath the workspace. A future network
artifact loader needs an explicit validation boundary, not silent runtime repairs.

Reuse is bounded, not a global memoization table. The shared effect VM remembers
one successful effect/time identity, while operator depth slots remember their
own node/time identities. Interleaved effects or temporal keys can require fresh
initialization; arbitrary-time semantics remain intact. No claim is made that every
possible signal graph avoids every repeated computation.

The review traced dense automation slots to elaboration and their reserved storage,
checked cache reset/error paths against seek and nested-sampling parity tests,
and checked source-span order/bounds and borrowed-buffer ownership. No new serious
correctness defect was found in those reviewed paths. Serial transport reliability
and exhaustive validation of arbitrary manually constructed artifacts remain gaps.
