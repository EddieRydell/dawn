# Runtime optimization results — 2026-09-04

## Current consolidated result

The optimization implementation and laptop/ESP32 measurements are complete.
The sections after this summary are historical checkpoints: their pending-work
lists describe that checkpoint, not the current runtime.

The latest saved board capture is
[`2026-09-04-array-storage.txt`](../firmware/dawn-profile/results/2026-09-04-array-storage.txt):
160 cases, matching host checksums, zero timed allocations, zero prepared
first-frame allocations, and all 163,840 configured heap bytes recovered.
Its firmware ELF SHA-256 is
`e89288566a3cf482d288cef6864356dea40c6e04aaf3874ed6e1ba08f68ce5fa`.
Release build/clippy and full repository checks passed after the final runtime
change. The completion audit additionally refreshed firmware formatting/clippy
and the linked-image software-double direct-call scan (zero references).
A fresh completion-audit `pnpm check` also exited successfully, including
frontend checks/build/tests and workspace formatting/check/tests/clippy. Vite
still reports its existing large-bundle advisory; it does not fail the build.

### Performance from the original baseline

ESP32 at 240 MHz, one core, radio off, no physical output; median prepared-frame
time for 200 pixels (600 RGB channels):

| Effect | Original | Current | Speedup |
| --- | ---: | ---: | ---: |
| ScanSweep | 8.781 ms | 5.249 ms | 1.67x |
| ImpactBurst | 8.892 ms | 5.447 ms | 1.63x |
| SparkleComet | 33.050 ms | 8.315 ms | 3.97x |
| ShimmerField | 22.343 ms | 5.239 ms | 4.26x |

The original four cases retain the same two-buffer graph. Math and code placement
both changed; these are combined gains, not attribution to a single optimization.
Random output intentionally changed, and approximate trig is not bit-identical
to the original implementation.

The latest full desktop Criterion comparison reports 489.79 us for the VM batch
versus approximately 503 us originally (about 3% less time), and 8.9356 ms for
60 controller frames versus 13.17 ms (32% less time). Preparation is 556.08 us;
it is not the optimization target. The preceding controller run was 8.7411 ms:
the current result is 2.2% slower, while direct VM and temporal results remained
nearly unchanged. The intervening runtime edit only increases preparation-time
capacity for empty curves; these runs do not establish a per-frame causal regression.

New benchmarks were introduced during the goal, so their first results are not
original-goal baselines. Calculated-array layered evaluation improved roughly
2.2x from its first measurement. Same-image nested-operator controls demonstrate
26-27% less time with prefix reuse. Cross-image nested timings varied materially
even without runtime source changes; this supports executable-layout sensitivity,
not a measured cache-miss rate or universal speedup.

### Requirement and evidence audit

| Requirement | Current implementation and verification |
| --- | --- |
| One stateless, embedded-capable engine | `dawn-runtime` is `no_std`; firmware imports it with default features disabled. Effects/operators sample prepared signals at time t. Forward/backward seeks and fresh/reused workspace comparisons protect independence from evaluation history. |
| No software doubles in sampling math | Approved micromath supplies f32 trig; deterministic random uses a 32-bit hash. Reachable libm helpers were inspected, with current ELF direct-call verification as corroboration. Accuracy tests cover the documented phase ranges. |
| No successful prepared-frame heap allocation | Workspace construction reserves registers, arrays, curves and graph buffers. Nine allocation tests cover calculated arrays, enums, temporal reads, native automation, empty curves, mixed native/DSL nodes and preview reads; the 160-case device capture checks first and subsequent prepared frames. |
| Move fixed work out of evaluation | Compiler array lowering/slot selection, copy/dead-value cleanup and repeated signal-read elimination; elaboration target routing, output aliases and RGB lookup tables. Bounds/error, routing and exhaustive packing tests preserve behavior. |
| Reuse time-dependent uniform calculations | Compiler emits a numeric initialization prefix; runtime reuses it per effect/sample time, including nested operators. Tests compare against full execution across mutable locals, sibling parameter sets, changed times and revisits. |
| Layered and operator performance | 1/4/16-layer fixtures, two-time reads, mixed native/DSL chains and depths 2/4/8 are measured. Same-image prefix controls isolate reuse; arbitrary graph cost is not bounded by these fixtures. |
| Regressions investigated | Controlled IRAM placement, same-image prefix controls and repeated captures distinguish observed changes from attribution. Small remaining differences are reported rather than hidden or blindly reverted. |
| Laptop and physical ESP32 validation | Full Criterion comparison, full repository checks, standalone release firmware checks, and saved checksum/allocation-checked COM4 capture. No LEDs were connected or driven. |

### Contract and remaining limitations

- The allocation contract is **successful sampling of compiler/elaboration-produced
  data with its matching prepared workspace and caller-owned output buffers**.
  Startup, binding, generator export, owned-result convenience APIs and error
  strings can allocate. Public hand-built/mutated prepared structures are not a
  validated untrusted-input format. This is not a heapless crate.
- Array bounds remain conservative. Shared enum/resource handles still perform
  reference counting; some name comparisons and per-pixel VM dispatch remain.
  Hoisting is deliberately conservative around mutation, references and errors.
- Memory is not universally reduced: the original 200-pixel cases retain
  roughly 184-268 additional bytes. The node-indexed operator cache reduced a
  representative 200-pixel workspace from about 21.9 KB to 9.6 KB. A 1600-pixel,
  16-layer case retains about 148.5 KB, leaving only about 15 KB of configured
  heap; this is not evidence of adequate Wi-Fi headroom.
- Nested operators use recursion. Depth eight is exercised on hardware, but
  stack high-water and a safe production nesting limit are not established.
- Trig maximum absolute error measured over +/-25,000 radians is 0.003294289;
  arbitrary phases and amplified downstream effects have no such guarantee.
  Timeline storage uses integer microseconds; authored numeric seconds expressions
  still use f32 and can round.
- This is profiling firmware, not a production network receiver, serialized-show
  loader, synchronized clock service or LED driver. Compute timings exclude those
  systems and do not establish end-to-end frame deadlines.

### Source size and simplicity

The same tracked directories now contain **16,653 nonblank Rust lines in 54
files**, versus 15,088 in 52 files originally: **+1,565 lines (+10.4%)**.
This count includes in-module tests and firmware workloads, but excludes standalone
tests, benches, build scripts and documentation. Moving shared fixture code from
an excluded test into firmware also affects this accounting.

The work removes repeated runtime calculations and several old mechanisms, but
does not achieve net source removal. Compiler analyses, bounded array ownership
and expanded profiling introduce real complexity. There is still one evaluator,
not a separate embedded engine; that architectural simplification must not be
confused with a smaller implementation. The code-size increase is an explicit
tradeoff of this optimization, not an unreported simplification success.

## Initial implementation checkpoint

- Approved `micromath` 2.1.0, no default features, for the shared VM's sine and cosine. The old `libm` f32 trig API performed software-double polynomial arithmetic on ESP32. Rounding and the existing f32-only gamma implementation remain in `libm`.
- Stateless random sampling now hashes the f32 seed bits with the MurmurHash3 32-bit finalizer and scales its upper 24 bits into [0, 1). Signed zero is normalized. Seeded random outputs intentionally change. This is not a cryptographic RNG.
- Compiler lowers assigned parameters into initialized locals. Removed `StoreParam`, the VM override vector, lookup branches, setup reservation, and per-invocation override cleanup. Branch/loop assignment and invocation independence have regression coverage.
- Compiler emits conservative `uses_pixel_context` metadata. Whole effects without pixel reads or upstream signal samples evaluate once per effect/sample time, then broadcast. This is NOT subexpression-level time-invariant hoisting yet. Tests cover dependency detection and forward/backward seeking with layered output.
- Elaboration aliases a terminal single-input graph output to its input frame slot instead of adding a clear/composition pass and a buffer. Multi-input output still composes normally.
- Immutable identifier strings are shared using the existing ownership mechanism (Rc in the non-atomic ESP32 build, Arc on desktop). Generic enum copies and constant loads no longer allocate strings. This is NOT numeric enum interning; name comparisons and reference counting remain. Removed enum string-capacity reservation machinery. The allocation regression test exercises local assignment without warming up the longer enum name.
- Firmware uses esp-hal's supported rwtext linker hook to place the VM interpreter and specialized show evaluator in instruction RAM. Runtime code itself has no target-specific implementation. This consumes instruction RAM; it is not a free memory optimization.

## Measurement evidence

Original baseline is `firmware/dawn-profile/results/esp32-v3.1-2026-09-04.txt`.
Intermediate complete captures are `2026-09-04-math-iram.txt`, `2026-09-04-compiler-iram.txt`, and `2026-09-04-layered.txt` in the same directory. The latter predates the shared-Identifier change.

The subsequent `2026-09-04-shared-enums.txt` capture records the shared-Identifier checkpoint:
64 unique records, zero warmed allocations, zero mismatched frames, and the
full configured heap free at completion. Its ELF SHA-256 is
`1e06f9443282e647fc76c2ec0dfa474ad54b6683e385e43c0c0d6a5388d7db32`.
At 200 pixels its full-frame times are 5.915/6.011/9.461/6.705 ms for
ScanSweep/ImpactBurst/SparkleComet/ShimmerField respectively. UniformFade takes
0.212/0.611/2.039 ms at 1/4/16 layers; PixelRamp takes 0.927/3.472/13.485 ms.
Shared string headers add 24–56 bytes of retained heap in the four original
fixtures compared with the prior compiler checkpoint; eliminating copy churn
is not the same as reducing every startup allocation or retained byte count.

All measured firmware runs use one core at 240 MHz, radio off, no GPIO output, internal heap 160 KiB, 32 varied sample times per case. Frame checksums are outside timing. No LEDs are connected.

At 200 pixels, after math changes, compiler parameter lowering, and both hot functions in IRAM (before shared identifiers):

| Effect | Original full frame | Updated full frame |
| --- | ---: | ---: |
| ScanSweep | 8.782 ms | 5.915 ms |
| ImpactBurst | 8.895 ms | 6.052 ms |
| SparkleComet | 33.064 ms | 9.557 ms |
| ShimmerField | 22.084 ms | 6.762 ms |

The original four fixtures deliberately keep their original two-buffer graph, so these comparisons do not silently include the new single-output alias. Changes combine math and code placement; do not attribute the whole speedup to one instruction.

The first math-only flash build regressed ScanSweep to 16.716 ms despite unchanged effect arithmetic. Moving the interpreter to IRAM reduced it to 6.292 ms. Subsequent VM changes improved direct sampling but the flash-resident show wrapper regressed to 7.135 ms. Moving that wrapper to IRAM reduced the same source path to 5.915 ms. The expanded six-effect binary reproduced that 5.915 ms result. This supports a code-placement issue, but is not a hardware cache-miss counter measurement.

### Layered workloads

The harness now measures six effects: the original four plus UniformFade and PixelRamp. The added effects run at 1, 4, and 16 fully overlapping layers, across 200/400/800/1600 pixels. Inputs are intentionally identical, so max-composited output equals the independent direct-VM golden output. This isolates layer/coverage cost, not a realistic mix of different programs or temporal operators. The collector requires 64 unique measurements.

At 200 pixels, full output (before shared identifiers):

| Workload | 1 layer | 4 layers | 16 layers |
| --- | ---: | ---: | ---: |
| UniformFade | 0.212 ms | 0.618 ms | 2.068 ms |
| PixelRamp | 0.951 ms | 3.564 ms | 13.848 ms |

At 1600 pixels/16 layers, UniformFade takes 15.463 ms and PixelRamp 110.311 ms. Retained heap is about 148 KB, leaving about 16 KB of the configured heap. This is not enough evidence of Wi-Fi-ready memory headroom.

Desktop Criterion baseline `effect-vm` was saved before edits. The original four-effect batch measured about 503 us. After math/compiler changes, a full comparison measured about 464 us; controller dense playback remained near 13.3 ms per 60 frames versus 13.17 ms before, within the benchmark noise threshold. Later comparisons must be reported separately rather than treating quick-pass timing as a settled gain. New `prepared_layers/*` baselines were established after introduction, not on the original source.

The shared-Identifier full comparison measured 451 us for the VM batch
(Criterion estimate -9.6%), 13.160 ms for controller dense playback (-0.1%, no
detected change), and 576 us for preparation (+8%, detected regression).
Immutable string construction adds an allocation/header at startup and removes
subsequent copy allocations; that is a concrete changed cost, not an isolated
proof of the entire preparation regression. Do not revert on this number alone.

## Initial correctness checkpoint and then-remaining work

Full `pnpm check` and firmware release clippy pass at the shared-Identifier checkpoint. Math tests check sine/cosine against libm with absolute error <0.002 over [-1000, 1000] radians; this is not an all-input accuracy guarantee. Random tests check repeatability, bounds and a coarse bucket distribution. Cross-device checksum equality is separate from old-output parity: math/random outputs intentionally changed.

Still required by the active goal:

- Reduce the conservative calculated-array memory bound and scalarize fixed-index cases. The compiler now bounds array storage from reference slots, nesting depths and construction widths. The VM uses preallocated slots managed by the existing offset allocator, with non-atomic ownership counts for aliases and nested children. Generator output materializes owned arrays outside prepared sampling. Laptop lifetime and allocation tests pass, including the first prepared frame, and all 80 ESP32 cases complete with zero timed allocations and matching host checksums.
- Move fixed expression work into compilation/elaboration and hoist pixel-independent subexpressions once per sampled time, not merely wholly uniform effects. Avoid introducing another execution engine.
- Simplify show/patch color copying and precompile more routing; preserve preview element state and fixture/control behavior.
- Address repeated scalar graph sampling/target searches and temporal/DSL operator workloads; these are not covered by the new layer-only benchmarks.
- Validate trig accuracy over larger show phases and audit all reachable math helpers, not merely source-level f64 searches.
- Investigate any persistent laptop regression with comparable, uncontended measurements; do not revert just because a number changed.
- Final board/laptop checks, final allocation guarantee, and final LOC/memory report after the whole overhaul.

The calculated-array test previously reported 20/268/40008 allocations across
two samples with 2/64/9999 loop iterations. With reusable storage it reports
zero in all three cases and is no longer ignored. A separate four-layer test
checks zero allocations from the first prepared frame, including backward seeks.
Full repository checks and firmware release clippy pass at this checkpoint.

The expanded trig sweep over +/-25,000 radians measured maximum absolute error
0.003294289 against libm. The test retains <0.002 within +/-1,000 and checks
<0.004 over the wider range. This characterizes f32 range-reduction error; it
does not promise accurate results at arbitrary magnitudes or after downstream
effect amplification. No extra per-sample range-reduction path was added.

The post-array desktop VM suite measured 466.50 us (Criterion -7.17% versus the
original baseline), versus 451.09 us at the shared-Identifier checkpoint. These
separate runs do not establish the exact array-support overhead. The new
ArrayRamp benchmark deliberately exposes construction and indexing work that
should eventually be scalarized by compilation, not hidden by uniform caching.

Three fresh ESP32 captures failed strict ASCII validation, with the third
showing a 64-byte 0xff prefix replacing part of a measurement line. They are
incomplete and are not checked in as successful results. Early intact records
show ScanSweep at 5.944 ms for 200 pixels, zero allocations/checksum mismatches;
this was not full-run or array-workload verification. The fourth attempt
completed unchanged firmware and passed all 80 records; the corruption's cause
is not established. Complete capture: `firmware/dawn-profile/results/2026-09-04-calculated-arrays.txt`.

The flashed ELF SHA256 is
`efd4869c49b58f9c0456820f5996fdab959a6e46276f8b9450a4a3f6333ba61a`.
Both VM execution (29,456 bytes) and PreparedShow evaluation (17,865 bytes)
remain in IRAM. Heap returns to 163,840 bytes free after the run. All prepared
first-frame allocation counts are zero too; direct VM startup still allocates
its workspace as expected.

At 200 pixels, ArrayRamp show evaluation is 4.836 ms versus PixelRamp's 0.936 ms;
four layers are 19.079 versus 3.517 ms, and sixteen are 75.883 versus 13.670 ms.
ArrayRamp retains 2,192 additional bytes in each equivalent case, independent
of pixel/layer count, because effect VM storage is shared. The 1600-pixel,
16-layer array case retains 150,088 bytes and takes 606.520 ms. Zero allocation
is emphatically not sufficient for good speed or a practical Wi-Fi heap budget.

The new laptop layer benchmark baseline `calculated-arrays` measures ArrayRamp
at 50.137/200.61/800.05 us for 1/4/16 layers, versus PixelRamp at
7.1233/28.542/111.76 us. It is a current comparison between equivalent outputs,
not a before/after measurement of the old heap-allocating array implementation.

Additional discovery to handle explicitly: the existing DSL treats `srand(x)` as a pure hash-returning call, not a seeding side effect, while the two sparkle examples discard its return and subsequently call `rand()` without arguments. Those calls hash zero. No hidden causal random state should be added; changing the example programs to explicit seeded sampling would be a separate behavioral correction and changes benchmark inputs.

## Fixed-index array lowering checkpoint

`dawn-language/src/dsl/array_lowering.rs` now replaces known array indexing and
length reads within a basic block, follows aliases, and removes unused array
construction/copies. It does not substitute mutable registers for array
snapshots, propagate facts across control-flow boundaries, remove bad-index
errors, or remove potentially failing element computations. Jump targets are
remapped after removal. Array storage is zero when no MakeArray remains. This
is host compilation only: there is no new runtime instruction or engine.

The focused tests cover optimized fixed indexing/aliases/length, mutable scalar
snapshots, if/else merges, loop-carried arrays, shadowing, dynamic indexing and
bad indices. Existing nested array and generator lifetime tests still pass.
Full `pnpm check` passes after replacing a compiler `expect` with a checked
length guard. Firmware release build/clippy/format pass. The guard-only rebuild
is byte-for-byte identical to the flashed image:
`2d0cf6bde4888b17bf4fac89da67a16fd8cd7d44125b13561f997ca587f0f38a`.

Complete board capture: `firmware/dawn-profile/results/2026-09-04-array-lowering.txt`.
All 96 cases pass host checksums with zero timed allocations, and all prepared
first frames allocate zero too. The full heap is recovered after the run.

At 200 pixels, ArrayRamp full-show timing changes from 4.836 to 1.697 ms;
four layers from 19.079 to 6.561 ms; sixteen from 75.883 to 25.844 ms. Each case
retains 1,860 fewer bytes. PixelRamp remains 0.936/3.517/13.670 ms, so equivalent
scalar code is still faster. The new DynamicArray workload exercises actual
storage and dynamic indexing (4.492 ms for one layer/200 pixels).

The desktop repeat without concurrent builds measures ArrayRamp at
13.316/52.221/205.68 us for 1/4/16 layers, versus the saved calculated-array
baseline of 50.137/200.61/800.05 us (roughly 74% lower time). PixelRamp measures
7.0415/27.262/107.91 us, within the configured noise threshold versus its
baseline. An earlier run overlapped validation and reported a scalar
regression; that did not reproduce without concurrent builds. UniformFade
also shows no meaningful change. Do not use the contended run to attribute a
regression to compiler lowering. The original baseline files were preserved.

The direct ArrayRamp VM measurement is 3.972 ms at 200 pixels, despite the full
show taking 1.697 ms. That discrepancy is real in this capture and is not an
allocation difference; its cause is not yet isolated. The VM dispatch and show
wrapper remain in IRAM, while run_sample_program remains in flash. Placement
or caller specialization is a hypothesis, not an established explanation.

Remaining generated bytecode includes dead index constants, an unused constant
array load, redundant scalar moves, and unused reference register/pool entries.
These explain why removing MakeArray is not the final compact representation.
The next compiler cleanup must preserve errors and invocation-local snapshots,
and should serve general constant folding/hoisting rather than special cases
for the benchmark. Cross-block array analysis is still conservative.

## Bytecode cleanup checkpoint

Compilation now propagates same-type register copies within a basic block,
removes unused constant loads/copies/array construction, and compacts the
constant pool, operand spans, generator emission fields and typed register
layout. Reassignments invalidate copy facts; branch boundaries clear them.
Operations that can fail or sample signals are retained even when unused.
Array lowering no longer owns separate removal/dependency logic. One exhaustive
register-operand description in the host compiler serves cleanup and remapping;
no new runtime instruction or runtime code was introduced.

The ArrayRamp and PixelRamp programs now have identical instructions, constants
and register layouts. A test enforces this, and another
checks that an unused array containing integer division-by-zero still fails.
ArrayRamp has five instructions, one float constant, three float registers and
one color register; it has no reference/integer registers or operand storage.

Complete board capture: `firmware/dawn-profile/results/2026-09-04-bytecode-cleanup.txt`.
All 96 cases pass checksums with zero timed allocations. Prepared first-frame
allocation counts are also zero, and all 163,840 heap bytes return at the end.
ELF SHA256: `f6750ea85fc908d7566212225c22864cd8b1a5f18edd3d0de702d1146190ed3b`.
The final compiler slice-signature cleanup produces an identical ELF to the
flashed and measured image. Full `pnpm check` passes.

ArrayRamp now exactly matches PixelRamp's board timing and retained memory:
0.936/3.517/13.670 ms at 200 pixels and 1/4/16 layers. It saves another 332 bytes
versus the preceding array-lowering checkpoint, for a total 2,192-byte saving
versus reusable arrays without lowering. Direct VM timing is also identical
(0.847 ms at 200 pixels), removing the previously observed VM/show discrepancy
for this case. The underlying cause of that earlier discrepancy was not
isolated independently of removing the unused reference operations.

DynamicArray remains on the general storage path (4.492 ms at 200 pixels), with
unchanged timing and memory. The original four effects differ by about eight
microseconds per frame from the preceding board image; this is small, nearly
constant across pixel counts, and is not evidence of a scaling compute
regression. The VM engine source is unchanged in this step.

General constant folding and subexpression hoisting remain unfinished. This
cleanup deliberately preserves arithmetic and context operations rather than
assuming all unused expressions are harmless. Cross-block analysis and array
storage bounds remain conservative; dynamic arrays still have substantial cost.

## Array call-site guard checkpoint

The VM now checks `array_capacity` before calling the out-of-line storage
reservation function during sampling. This avoids calling into allocation/setup
code for programs that need no calculated arrays; array support is retained.
The complete `2026-09-04-array-guard.txt` capture passes all 96 checksums, zero
timed allocations and zero prepared first-frame allocations, with full heap
recovery. Full repository and firmware checks pass. Flashed ELF SHA256:
`4e0876312b7a6e1377187166762dc33f6958ef421da6c0eb95ca687469f7f166`.

Compared with the immediately preceding image, ScanSweep saves about
9/18/37/73 us at 200/400/800/1600 pixels. PixelRamp and ArrayRamp return to
0.927/1.845/3.681/7.352 ms for a single layer at those pixel counts, matching
the pre-array-storage PixelRamp timings. At 200 pixels the 4/16-layer cases
take 3.480/13.524 ms. This supports the unnecessary per-sample call as a real
ESP32 cost. DynamicArray changes from 4.492 to 4.495 ms at 200 pixels: it pays
the additional guard and still performs storage checks. Memory use is unchanged.

The controlled desktop PixelRamp/1 baseline `before-array-guard` measured
7.2165 us; the guarded build measured 7.4334 us (Criterion +4.72%, within its
configured noise threshold). The guard does not demonstrate a desktop win.

The complete `pnpm bench:effect-vm:compare` passed before the guard, with
457.78 us for the VM suite (Criterion -9.26% versus the original baseline).
ArrayRamp measured 6.9037/27.295/107.54 us for 1/4/16 layers. This is roughly
86% less time than the pre-lowering `calculated-arrays` baseline. Missing
`effect-vm` baselines for ArrayRamp and DynamicArray were initialized at this
optimized checkpoint only; they are not pre-overhaul baselines. All original
baseline entries and the historical `calculated-arrays` baseline were retained.

Real-project rendering remains slower than the original baseline. Before the
guard, representative frames measured 4.6720 ms (+8.04%) and dense playback
13.409 ms (+7.42%). A focused guarded repeat measured 4.8015 ms (+11.03%),
13.536 ms (+8.43%), and controller output 14.140 ms (+7.34%). These are observed
regressions, not evidence that the guard caused their full size. PixelRamp also
remains slower against its older `effect-vm` baseline than against the newer
`calculated-arrays` baseline; those comparisons must not be conflated.

The fixture uses native spin/pulse effects plus a DSL TimeWarp operator. The
scalar graph/operator path still performs per-pixel VM/context work and
upstream resampling. Its remaining overhead and code/cache sensitivity need
isolation; array elimination and the call-site guard do not resolve the desktop
regression. Do not mark the goal complete or hide these cases by removing them.

## Color-buffer clear checkpoint

`PreparedShow::evaluate` no longer clears plain color elements before copying
sequence colors into them. Each prepared color span overwrites its whole
element. Unmapped color elements remain black from workspace creation because
neither controls nor fixture behavior rules can write plain color elements.
Scalar, indexed and fixture state resets are unchanged. This requires no new
prepared metadata, allocation, buffer, or runtime abstraction.

The new starter-project test compares reused and fresh workspaces and patched
bytes across backward seeks, active frames, every effect end time, zero and
sequence end, including an added unmapped color element. It passes along with
the controller allocation test, full `pnpm check`, and firmware release build,
clippy and formatting checks.

An uncontended Criterion comparison against the immediately preceding
`before-color-clear` baseline measured controller output at 14.781 ms versus
14.692 ms (no detected change), and PixelRamp/1 at 7.1891 us versus 7.3489 us
(within the configured noise threshold). This does not resolve the older
desktop real-project regression.

The complete `2026-09-04-color-clear.txt` board capture passes all 96 checksums,
with zero timed allocations, zero prepared first-frame allocations, unchanged
retained memory and full heap recovery (163,840 bytes). PixelRamp's single-layer
median is 923/1838/3667/7325 us at 200/400/800/1600 pixels, versus
927/1845/3681/7352 us before removing the clear. Direct VM timing is unchanged.
Layered timing also changes slightly; because the removed show clear runs only
once per frame, do not attribute all layered changes to its byte writes alone.
The first flash attempt failed with a transfer checksum error; the retry
succeeded. Flashed ELF SHA256:
`dc9b156609f917bd0303eb6a0e4b88d27495706d2af1f652396eeaaab7472b30`.

### Preparation opportunity identified at this checkpoint

The existing RGB packing pass stops when it encounters a dimming or scale
filter. Consequently `PreparedFilter::DimmingCurve` still reaches
`apply_dimming_curve` and `libm::powf` for gamma-corrected components during
playback. For RGB8 sources with component-wise fixed transforms, elaboration
can evaluate all 256 possible channel inputs and fuse a lookup table into
packing without approximation. Reorders and multiple transform order must be
preserved, shared intermediate readers must remain valid, and continuous
scalar/RGBW paths must not silently be quantized to RGB8. This optimization is
implemented in the next checkpoint below. General VM subexpression hoisting
and temporal-operator work remain open.

## RGB lookup checkpoint

The existing RGB packing pass now includes fixed dimming and scale/invert
filters. Elaboration runs the existing filter evaluator over all 256 possible
normalized channel values, in original transform order, and quantizes the
result into a 256-byte lookup table. Reorders are composed separately because
these transforms apply identically to each channel. An identity table is
discarded, leaving the original direct packing path.

Runtime packing branches once per filter invocation between direct copying
and table lookup. Eligible chains lose their float component buffers, filter
passes and per-channel gamma calculations. Shared intermediates, RGBW and
continuous scalar paths are not silently changed into RGB8 lookups. General
gamma evaluation remains available for those unfused paths.

The packing tests compare all 256 channel values, all six RGB orders, gamma
0.5 and 2.2, custom curves, and multiple interleaved scale/invert/gamma filters.
Shared-reader and RGBW preservation tests remain. The paired profiling fixture
uses gamma 2.2 on PixelRamp with the original filters versus a host-built table;
the build script independently verifies both against the same checksum set.
It constructs known prepared fixtures, not a serialized-artifact loader.

Full `pnpm check`, firmware release build and clippy pass. The complete
`2026-09-04-rgb-lookup.txt` capture has 104 matching records, zero timed
allocations, zero prepared first-frame allocations, and 163,840 bytes free at
completion. Flashed ELF SHA256:
`84d58209df7d8857e7b973c49b99056980c15cc43d3c91fcb8f55dc3581bbda4`.

| Pixels | Gamma filters, median | Lookup, median | Retained bytes, filters | Retained bytes, lookup |
| --- | ---: | ---: | ---: | ---: |
| 200 | 4.315 ms | 0.940 ms | 14,116 | 9,400 |
| 400 | 8.617 ms | 1.871 ms | 27,316 | 17,800 |
| 800 | 17.212 ms | 3.734 ms | 53,716 | 34,600 |
| 1,600 | 34.396 ms | 7.459 ms | 106,516 | 68,200 |

These are whole-show evaluation times for PixelRamp, not isolated gamma calls.
They exclude GPIO/network output. The table adds 256 retained bytes compared
with plain packing; removing component workspaces and steps saves substantially
more. Existing non-gamma cases retain the same memory use and broadly similar
timings. Small timing shifts in unrelated cases are not credited to gamma
fusion. In particular, the dynamic-array case at 200 pixels changes from
4.492 to 4.515 ms; its direct VM also changes from 4.406 to 4.415 ms despite
unchanged VM source. This remains a code-placement/timing observation, not a
measured cache-miss explanation or a reason to revert the optimization.

The laptop's paired Criterion cases (same binary, complete show evaluation)
measure raw versus lookup at 30.590/7.2241 us for 200 pixels,
59.446/14.494 us for 400, 121.69/28.194 us for 800, and
238.58/57.084 us for 1,600. The 4.1-4.3x improvement comes from removing
the gamma/filter work, not from a changed effect. All varied-frame checksums
are compared before timing. Only these newly added `prepared_gamma` entries
were saved into the `effect-vm` baseline; older baseline entries were retained.

A subsequent non-gamma comparison against `before-color-clear` measures
controller output at 14.202 ms and PixelRamp/1 at 7.0143 us. Both changes are
within the configured noise threshold. This neither demonstrates a new
non-gamma regression nor resolves the older original-baseline render regression.
The current code has completed its checks and board run; general VM
subexpression hoisting, temporal-operator optimization and the broader
allocation/reachable-math verification remain unfinished.

## Uniform initialization prefix checkpoint

Compilation now uses the existing exhaustive operand visitor to find supported
pure, single-assignment numeric expressions whose inputs do not depend on a
pixel. Those instructions move into a topologically ordered initialization
prefix; jump targets are remapped into the remaining body. Mutable registers,
references and potentially failing operations are not lifted. Parameter loads
are lifted only from the original entry block. Generator programs are unchanged.

`BytecodeProgram::pixel_entry` records the body's first instruction. During one
effect's frame traversal, the first sample runs from zero and later samples
start at `pixel_entry`. Existing numeric registers hold the prefix results;
there is no new cache buffer, cross-frame key, persistent time state or second
interpreter. The flag resets for each effect/time, including seeks and automated
parameter updates. Single-pixel APIs and recursive scalar signal sampling still
run the full program. DSL operator frame traversal does not yet reuse its prefix.

The eight firmware programs lift 9/8/13/21/5/2/2/7 instructions respectively
(ScanSweep, ImpactBurst, SparkleComet, ShimmerField, UniformFade, PixelRamp,
ArrayRamp, DynamicArray). Tests compare mixed time/pixel expressions to scalar
sampling across layers and backward seeks, including mutable parameters,
branches, loop backedges, dead error branches and calculated arrays. Full
`pnpm check`, firmware release build/clippy, and the complete laptop Criterion
comparison pass their correctness gates. Timing regressions remain visible.

The first complete board capture, `2026-09-04-uniform-prefix.txt`, passes all
104 records with zero timed/prepared-first-frame allocations and full heap
recovery. It adds four retained bytes per program in these show fixtures.
Its ELF SHA256 is
`7cf85b5b678170b4885152ebb3098acf2e4e636c425b6810b285c35a1ba3e969`.
At 200 pixels the preceding/current whole-frame medians are:

| Effect | Before prefix reuse | With prefix reuse |
| --- | ---: | ---: |
| ScanSweep | 5.931 ms | 5.289 ms |
| ImpactBurst | 6.025 ms | 5.552 ms |
| SparkleComet | 9.504 ms | 8.418 ms |
| ShimmerField | 6.728 ms | 5.352 ms |
| PixelRamp | 0.923 ms | 0.799 ms |
| DynamicArray | 4.515 ms | 4.026 ms |

Direct DynamicArray sampling instead regresses from 4.415 to 5.595 ms at 200
pixels, and from 35.293 to 44.789 ms at 1,600. Its prepared path improves, so
this is not evidence of added allocation or more executed pixel instructions.
The sampling helper is still flash-resident in this first image. A controlled
placement experiment follows, without changing bytecode or VM logic.

The full laptop run measures the direct VM suite at 494.03 us (no significant
change against its original baseline), representative render at 4.8469 ms
(+12.09%), dense playback at 13.921 ms (+11.52%), and controller output at
14.358 ms (+8.99%). These regressions are against the older original baselines,
not an isolated before/after measurement of this pass. PixelRamp/1 is 7.0791 us
versus 7.0143 us in the preceding checkpoint: no demonstrated desktop gain.
DynamicArray/1 is 39.247 us. The earlier quick smoke estimates are not substituted
for these full measurements.

Disassembly shows the desktop sampling helper still creates/drops a VM per
pixel: eight pushed registers and a 0xf8-byte stack allocation in this build,
with out-of-line constructor and run calls. Prefix reuse removes expressions,
not that setup cost. This identifies remaining work, not a proven explanation
of the entire historical desktop regression. Broader pure-operation coverage,
operator reuse, constant folding and allocation/math verification remain open.

### Sampling-helper placement experiment

The completed `2026-09-04-uniform-prefix-helper-iram.txt` capture contains
104 records, all with zero timed allocations and zero mismatched frames. The
final free heap is 163,840 bytes. The captured firmware ELF SHA256 is
`8ec9415830eb6319b6f258bca15c220dd31030328ebbe7ec1a54ed1029ab2717`.
The only implementation difference from the first prefix image is placing
`run_sample_program` in instruction RAM through the linker fragment. Its
249-byte symbol moves from `40109cd0` to `40088f34`; this is not a measurement
of total IRAM cost including literals and alignment.

Direct DynamicArray at 1,600 pixels worsens from 44,789 to 54,231 us, while
prepared DynamicArray is essentially unchanged at 31,862 versus 31,865 us.
Moving a helper also changes flash layout, so this establishes placement
sensitivity, not the identity of a cache conflict or proof that the helper
itself is the bottleneck. The experimental linker placement remains in the
worktree and on the board; it is not a demonstrated general improvement.

Source inspection identifies avoidable work independently of that timing
experiment: `MakeArray` obtains a slot from the preallocated offset allocator,
copies elements into tagged `RuntimeValue` storage, manages reference roots,
and ultimately releases the slot. `Index` dispatches through generic value
conversion and target-kind matching before reading an element. These are
**not system-heap allocations**, but they still execute in the sample loop.
The DynamicArray fixture has a fixed length of three; only its indices vary.
A compiler-lowered indexed selection over existing typed value slots could
remove that array lifecycle for nonescaping fixed arrays. This is a next
implementation candidate, not an implemented optimization; mutable inputs,
aliases, nested arrays and index errors must retain their semantics.

No Rust source was changed in this placement-evidence checkpoint, so the
previous source LOC count and full source checks remain the relevant ones.
Neither the historical desktop regressions nor the overall goal are resolved.

### Fixed-array indexed selection checkpoint

`Select` reads a compiler-owned operand span and copies the selected typed VM
slot. Array lowering now emits this instruction when a basic-block-local array
snapshot has immutable element registers, including aliases. Generic cleanup
then removes unused `MakeArray` instructions. Mutable snapshots and values
crossing control-flow joins retain the existing array representation. Index
conversion and negative/out-of-bounds errors remain; unused selections are not
dead-code-eliminated because they can fail. Nested arrays can retain inner
storage even when the outer container disappears.

DynamicArray now has zero reference registers and zero calculated-array
capacity. Its operand pool contains three three-element spans rather than one:
this costs bytecode operand storage, but removes tagged array elements, slot
allocator metadata and reference lifecycle work from this fixture's samples.

Full `pnpm check`, the six array-lowering tests, allocation/uniform tests and
firmware release build/clippy pass. Board capture `2026-09-04-array-select.txt`
contains 104 records with zero mismatches/timed allocations and full 163,840-byte
heap recovery. ELF SHA256:
`b474eac59385c3404b704490db81779790c1515e8616ede4b3a79145cab1686c`.
The sampling-helper IRAM placement is unchanged from the preceding experiment.

| DynamicArray case | Before | Select |
| --- | ---: | ---: |
| 200 pixels, direct | 6,775 us | 2,299 us |
| 200 pixels, prepared | 4,026 us | 1,877 us |
| 200 pixels, 4 layers | 15,783 us | 7,246 us |
| 200 pixels, 16 layers | 62,638 us | 28,549 us |
| 1,600 pixels, prepared | 31,865 us | 14,894 us |
| 1,600 pixels, 4 layers | 125,661 us | 57,834 us |
| 1,600 pixels, 16 layers | 499,482 us | 228,233 us |

Retained memory falls by 1,364 bytes in each DynamicArray case. At 200 pixels
the prepared case uses 9,744 bytes versus 11,108 previously. These are compute
benchmarks with Wi-Fi off and no physical LEDs, not end-to-end output rates.
Other programs can still move in timing because interpreter code/layout
changes; the speedup is not claimed to apply to every effect.

The full laptop comparison completed successfully. The direct suite is
498.20 us, with no significant change against the original baseline. DynamicArray
at 1/4/16 layers measures 18.502/73.448/290.80 us, versus the preceding
39.247/155.44/612.96 us. Representative render is 4.7220 ms, dense playback
13.912 ms, cold playback 13.632 ms and controller output 14.185 ms. Historical
regressions remain against the original baseline (controller +7.68%, dense
playback +11.44%); this targeted improvement does not resolve them. PixelRamp/1
is 6.9364 us. These are full Criterion measurements, not quick-run estimates.

### Operator uniform-prefix reuse

DSL operator frame traversal now reuses its compiler-proven numeric prefix
after the first successful pixel. The existing detached operator VM holds the
slots; nested upstream sampling uses separate workspaces. Reuse resets on every
operator/frame invocation, so there is no persistent time key, cross-frame
cache or new engine. Scalar recursive operator calls still evaluate the full
program. The new test compares prefix reuse against prefix-disabled evaluation
for one and two nested operators, time-shifted signal reads, mutable gain,
pixel-dependent branches and backward seeks. All three prepared-uniform tests
pass. ESP32 release build/clippy and full repository checks pass. Operator-specific
board timing/coverage passes. The completed array benchmarks above
predate this operator change and are not evidence of its performance.

The shared workload now includes a sine-gain DSL operator over PixelRamp. Paired
`operator_full`/`operator_reuse` stages use identical code, parameters and graph;
only the program's prefix entry is set to zero in the full-evaluation case.
Host generation verifies equality across all 32 frames before emitting common
checksums. Firmware records both cases at all four pixel counts, bringing the
expected capture count to 112. Desktop Criterion has the same paired workloads
under `prepared_operator`; its new baselines are initialized without replacing
unrelated benchmark baselines. Full checks pass for this fixture expansion too.
The flashed ELF SHA256 is
`ea13923f075b78f0be8019f8ea0ca7d0e87cb7714760deadf27e108ec56eb7a4`.
The completed `2026-09-04-operator-prefix.txt` capture contains all 112 records,
with zero timed allocations/mismatches and full 163,840-byte heap recovery.
The paired cases have identical retained memory. Desktop Criterion also
completed all eight paired cases successfully.

| Pixels | ESP32 full / reuse | Desktop full / reuse |
| --- | ---: | ---: |
| 200 | 3,159 / 2,576 us | 27.114 / 24.870 us |
| 400 | 6,372 / 5,205 us | 54.331 / 50.521 us |
| 800 | 12,863 / 10,526 us | 110.22 / 103.08 us |
| 1,600 | 25,970 / 21,293 us | 227.89 / 211.66 us |

These isolate uniform-prefix reuse in a single same-time DSL operator, not
arbitrary time-warped or deeply nested graphs. Source inspection also identifies
the fixed 1,024-entry signal-cache reservation for even this one-input graph:
12,288 bytes on ESP32 before allocator overhead. Its sizing and recursive
insertion path remain candidates for simplification and allocation-bound review.

### Node-indexed signal cache

Signal memoization now uses a fixed boxed array with one optional latest-time
sample per graph node, allocated during workspace construction only when the
frame plan uses recursive sampling. Lookup and insertion index the node directly;
there is no growing vector, linear key search, or 1,024-entry budget/error.
Entries reset for each pixel. A different time replaces the node's previous
sample, which preserves stateless results but can require recomputation when a
graph revisits several alternating times. Clearing now scales with graph size;
this is another performance tradeoff to measure, not a universal speedup claim.

The new regression test samples 1,100 distinct upstream times, then revisits
current/earlier times, across backward seeks and multiple pixels. It matches an
independent source-only show and records zero allocations from the first frame.
Existing nested-operator and uniform-sampling tests pass. Firmware release
build/clippy and full repository checks pass. The full laptop Criterion
comparison completed successfully. The completed `2026-09-04-node-cache.txt` board capture
contains all 112 records, with zero timed allocations/mismatches and full heap
recovery. The paired operator workload retains 12,264 fewer bytes at every
pixel count (two 12-byte cache entries replace the old 12,288-byte reservation).
At 200 pixels total retained memory falls from 21,860 to 9,596 bytes. Prefix-reuse
frame time slightly regresses from 2,576 to 2,597 us; at 1,600 pixels it changes
from 21,293 to 21,419 us. This is a demonstrated memory saving, not a board
compute speedup. The exact timing contribution of indexed lookup, cache clearing
and code placement has not been isolated.

The completed desktop run measures the VM suite at 506.62 us, representative
render at 4.6197 ms, dense playback at 12.248 ms, cold playback at 12.149 ms,
and controller output at 12.551 ms. Controller output and dense playback are
back within the configured noise threshold against the original baseline.
Preparation remains 576.00 us (+7.18% versus original). DynamicArray/1 is
18.060 us; PixelRamp/1 is 6.3886 us. Paired operator full/reuse timings at
200/400/800/1600 pixels are 24.849/22.973, 51.273/46.786,
104.68/96.924 and 214.78/201.90 us. These measurements do not isolate the
contribution of cache data layout from generated code layout.
The flashed ELF SHA256 is
`92281af0cabaf6618f2d31d99fb92d26d2e98eb59ca310c19734040c45f9ae86`.

### Reusing initialized workspace

The existing nonzero prefix entry now also skips register-layout preparation
and calculated-array capacity checks. This path is private to traversal of one
program with its initialized workspace. Entry zero still initializes independent
samples, generators and the first pixel of each frame; programs with no lifted
prefix retain that initialization path. VM execution state still resets and
reference cleanup still occurs for each pixel. No new cache, engine or wrapper
is introduced. Focused allocation and prepared-uniform tests, full `pnpm check`,
and firmware release build/clippy pass. The board capture
`2026-09-04-init-reuse.txt` passes all 112 records with zero timed allocations,
matching checksums and full heap recovery. Retained memory is unchanged.
Operator prefix-reuse medians improve from 2,597 to 2,544 us at 200 pixels,
5,243 to 5,137 us at 400, 10,595 to 10,382 us at 800, and 21,419 to
20,992 us at 1,600. Full-evaluation operator cases remain approximately unchanged,
as expected because they initialize for every sample. The full desktop comparison
completed successfully. Operator reuse at 1,600 pixels measures 202.08 us versus
201.90 us at the previous checkpoint: no demonstrated desktop gain in that case.
The direct VM suite is 513.66 us; representative render is 4.5304 ms, dense
playback 11.756 ms, cold playback 11.658 ms, and controller output 12.179 ms.
Preparation measures 584.72 us. These are the initialization-reuse checkpoint,
before the equal-target routing and context-parameter validation changes.
Flashed ELF SHA256:
`b2ebba62e08f33ff87bcda77a5713e8d461519292c52ab5de2cb51231c8353b4`.
A disassembly scan found zero direct call/jump references to the common
double arithmetic, comparison and f32/f64 conversion helper symbols. This does
not cover indirect calls or all possible runtime programs and is not a complete
reachable-math proof.

The next routing candidate is the binary search in scalar layer sampling when
the effect target equals the graph target. Elaboration interns exact target
contexts (addresses, local indices/counts and pixel-fraction bits), and runtime
target IDs index the same pixel span, so that case already has the correct
flat pixel index. Different targets still require membership/address mapping.
This is now implemented: equal target IDs reuse the already resolved pixel;
different targets keep the address search. A new test forces identical contexts
under distinct target IDs and verifies both routes across backward seeks.

### Context-only parameter validation

The compiler rejected direct Timeline/Target/TargetItems/TargetItem parameters
but accepted arrays containing them. A regression test reproduced acceptance of
`array<Timeline>` before the fix. Validation now unwraps array element types
before applying the existing context-only restriction. The test covers all four
types at one and two array nesting levels. This closes a source-language path
for passing generator context resources into sampling; it is a compile-time
diagnostic change, not runtime sanitization of arbitrary hand-built bytecode.
All 26 focused DSL, allocation and prepared-uniform tests pass. Full repository
checks and firmware release build/clippy pass for these changes.
The completed `2026-09-04-target-route.txt` board capture has 112 records,
matching checksums, zero timed allocations and full heap recovery on ELF SHA256
`a04be74bf06a2e1566c5832eef26e6f4044cf770b3f489f3a298194745d59d24`.
Equal-target operator reuse medians improve from 2,544 to 2,226 us at 200 pixels,
5,137 to 4,441 us at 400, 10,382 to 8,870 us at 800, and 20,992 to
17,729 us at 1,600, without changing retained memory. The full desktop comparison
completed: VM suite 482.46 us, preparation 555.20 us, representative render
3.7782 ms, dense playback 11.572 ms, cold playback 11.429 ms and controller
output 11.965 ms. Criterion estimates versus the original baseline are -4.31%,
+3.89%, -12.63%, -7.30%, -10.21% and -9.17%, respectively; VM and preparation
are within the configured noise threshold. DynamicArray/1/4/16 measures
17.641/68.631/272.94 us. Operator full/reuse at 200/400/800/1600 pixels measures
20.768/18.575, 41.703/36.950, 82.411/73.046 and 164.83/146.82 us.
The newer fixture baselines are not original-goal baselines. These measurements
precede the following native automation fix and do not isolate code placement.

Further allocation review found that `prepare_effect_params` updates automated
curves before clearing the prior `native_sample`, whose raw curve reference can
therefore keep copy-on-write storage shared during the update. A focused native
Pulse/curve-automation regression test reproduced allocation counts [0, 2, 3, 2]
at forward/backward sample times before the fix. Releasing the previous native
sample before updating automation reduces those counts to [0, 0, 0, 0], with
output matching independent fresh-workspace evaluation at each time. All 27
focused DSL, allocation and uniform tests pass. The change moves one assignment
and adds an ownership-order comment, without adding a cache or abstraction.
Full `pnpm check` and firmware release build/clippy pass. Existing board fixtures
do not contain native curve automation, so hardware coverage remains required.

The profiling harness now adds four `native_automation` cases (200/400/800/1600
pixels), bringing its expected record count to 116. The host allocation test and
firmware use the same fixture constructor instead of duplicate setup. Golden
generation compares reused and fresh workspaces. Full repository checks and
firmware release build/clippy pass. The first flash attempt timed out; a retry
completed for ELF SHA256
`78452193d8113cf9a7796182fe4cc423bf3dd8adccaf50ad6270c87f881a5c61`.
The completed `2026-09-04-native-automation.txt` capture passes all 116 records,
with zero timed allocations, matching host checksums and full 163,840-byte heap
recovery. Every prepared first frame also reports zero allocations. Native
automation medians at 200/400/800/1600 pixels are 661/1297/2567/5109 us,
retaining 9,720/18,120/34,920/68,520 bytes. These are new workload measurements,
not a hardware before/after speedup claim. Forward/backward seeks remain covered
by the host allocation test; firmware's timed window advances sequentially.

### Native Pulse broadcast checkpoint

Native Pulse uses only progress and its bound gradient/curve, but the existing
whole-effect uniform check only recognized DSL programs. The check now also
recognizes the Pulse sample variant and reuses the existing color broadcast.
Other native variants remain pixel-dependent. No buffers or runtime abstraction
are added. The allocation regression now also compares native output against
equivalent DSL evaluation across forward/backward seeks. Focused tests and
firmware release build/clippy and full repository checks pass. The first capture
failed strict ASCII validation on a corrupt serial line and was discarded. The
fresh `2026-09-04-native-broadcast.txt` capture passes all 116 records, with zero
timed allocations, zero prepared first-frame allocations, matching host output
and full heap recovery. Native automation medians at 200/400/800/1600 pixels
improve from 661/1297/2567/5109 us to 226/425/823/1617 us (2.92-3.16x).
Retained memory is unchanged. The full desktop comparison completed: VM suite
482.04 us, preparation 563.23 us, representative render 3.4353 ms, dense playback
8.5178 ms, cold playback 8.6027 ms and controller output 9.0396 ms. Representative
render and controller output were 3.7782 ms and 11.965 ms at equal-target routing.
These are workload results, not isolated timing of native arithmetic. DynamicArray
1/4/16 measures 17.311/68.916/273.08 us; operator reuse/1600 measures 144.53 us.
ELF SHA256:
`58334f13b2105d8b0047a48216a236fe6f3b3169dce350165bfe8d929e545281`.
The tracked source count is 16,483 (+5 this step, +1,395 overall).

Dependency inspection confirms libm 0.2.16 `powf` uses f32 arithmetic and calls
`fabsf`, `scalbnf` and `sqrtf`. Its generic f32 square root uses widening integer
multiplication for the high product half; on this ESP32 ELF, the complete
`sqrtf` disassembly (0x40116a0c..0x40116af8) uses native `muluh`/`mull`, 32-bit
registers and single-precision instructions, with no function calls. This is
not software-double work or a reason to replace that dependency path. The
whole-image scan again found no direct calls/jumps to common double helpers;
ROM symbol definitions alone do not mean those helpers execute. Full indirect
call and all valid program-path coverage remains a separate requirement.

### Temporal cache measurement

The desktop operator harness now also defines grouped and alternating reads of
the same two upstream times, with four signal reads and identical max-composited
output in each variant. Checksums must match across all 32 sample times. This
targets the known one-entry-per-node eviction tradeoff without changing runtime
storage. It reuses the existing operator benchmark loop and adds no runtime code.
Full repository checks pass. A focused full Criterion run established only the
new `prepared_temporal` baselines, without overwriting existing workload baselines.
Grouped/alternating read times at 200/400/800/1600 pixels are 33.514/54.383,
66.369/107.89, 133.64/214.15 and 260.50/422.61 us. Alternating is 60-63% slower,
with matching checksums. This demonstrates a real repeated-evaluation cost for
this workload. The next candidate is compiler reuse of identical signal reads
within a basic block, invalidated on input/result writes and control-flow edges,
to remove redundant sampling without increasing runtime cache storage. That
optimization is now implemented in the existing copy/cleanup pass: matching
input and time-slot reads become moves from the prior color slot. Writes to
either slot and basic-block boundaries invalidate reuse. The first read remains,
including its errors; no reads are reordered and no runtime cache is added.
A recording-sampler test failed with a duplicate call before the change and now
passes cases for separate inputs, alternating times, changed times, branches
and loops. The SignalSampler contract now explicitly documents stateless results.
Firmware release build/clippy and full repository checks pass. The completed
full Criterion comparison measures grouped/alternating reads at 200/400/800/1600
pixels as 31.030/30.899, 61.624/60.991, 121.63/122.69 and 238.36/241.08 us.
Criterion estimates 43-44% less time for alternating reads and 7-9% less for
grouped reads, with matching output. This removes the measured ordering penalty
for repeated identical time slots without changing runtime cache storage.
The existing firmware fixtures do not exercise repeated temporal reads; board
measurement of this specific optimization and a full desktop regression run
remain required. The new firmware build has not been flashed in this checkpoint.
Arbitrary distinct times still require separate evaluation. Identical expressions
computed into different slots are not merged by this narrowly scoped change.
Tracked source count is 16,508 (+25 this step, +1,420 overall): compiler logic
and two runtime API documentation lines, not additional runtime instructions.

### Shared temporal board coverage

Grouped and alternating operator sources now live in the existing shared
profiling workload, used by laptop Criterion and host-generated ESP32 bytecode.
The firmware adds both modes at all four pixel counts, for 124 records. Golden
generation verifies their matching output; existing measurement loops and
program emission are reused. Full repository checks and firmware release
build/clippy pass. Flashing succeeded for ELF SHA256
`2e3cdc1a898cf7f78cc391761fdb28ceb9bb4b89b09c4dcf239d718ed44304af`.
The `2026-09-04-temporal-reuse.txt` capture passes all 124 records with zero timed
allocations, zero prepared first-frame allocations, matching host checksums and
full 163,840-byte heap recovery. Grouped and alternating temporal reads have
identical medians at each count: 3,981/7,944/15,879/31,747 us at
200/400/800/1600 pixels, retaining 9,596/17,996/34,796/68,396 bytes. These new
board cases have no pre-optimization hardware baseline; they verify current
output/allocation behavior, not a board before/after speedup. The full desktop
regression comparison completed: VM suite 480.06 us, preparation 550.22 us,
representative render 3.3387 ms, dense playback 8.1792 ms, cold playback
8.1906 ms and controller output 8.6907 ms. Temporal alternating/1600 measures
236.12 us. These results precede the following audit edits.
Tracked source count is 16,540 (+32 this step, +1,452 overall). This includes
moving source fixtures into the counted firmware directory; their removal from
the standalone benchmark is outside the historical count. Runtime code did not
change in this step.

### Audit checkpoint

- Proven preview allocation bug: `PreviewRenderer::update_colors` called
  `RenderedElementState::preview_colors()` for every binding and then selected
  one cell. For an N-cell color element with N bindings this allocated N vectors
  and copied N squared colors per update. The old vector-returning API is now
  replaced by `preview_color(cell)`, preserving bounds and scalar/fixture/indexed
  conversion without allocation. Callers and checksum tests use the single-cell
  API. A new allocation test covers all four element kinds and out-of-range cells.
  This is outside the ESP32 PreparedShow hot path; no LED speedup is claimed.
- Redundant VM state: `VmRegisterLayout` wrapped only `SlotLayout`, while
  `VmRegisters::prepare` also checked every actual register length. The wrapper,
  optional cached layout and duplicate equality check are removed; the five
  length checks preserve the existing preparation condition.
- Guarantee boundary: `PreparedShow::evaluate` and `PreparedSequence::evaluate`
  are the preallocated paths. `evaluate_frame*` returns owned per-element vectors,
  and workspace creation, binding, generator export and error-message construction
  still allocate. Zero allocations must not be claimed for every runtime API.
- Capacity evidence: sequence workspace construction reserves every assigned VM
  program and detaches/reserves automation curves. Show workspace reserves explicit
  fixture control addresses; element layouts reserve profile function capacity.
  Patch value layouts reserve filter/fixture widths. Calculated-array storage
  bounds use checked multiplication and fixed-width slots. Existing allocation
  tests and board cases support these paths, but do not replace the remaining
  all-path review and malformed-prepared-data boundary assessment.

The above edits pass all 29 focused DSL, allocation and uniform tests, full
repository checks and firmware release build/clippy. Subsequent flash and timing
validation is recorded below. Tracked source count is 16,529 (-11 this step,
+1,441 overall); standalone regression tests and preview renderer changes are
outside that historical count. No performance gain from these audit edits has
been measured yet; the previous full benchmark used the preceding executable.

### Layout cleanup validation

The smaller-register-workspace firmware flashed successfully with ELF SHA256
`e3f57490fd050dc768cac992cd911c8363af7ffc789119cbd6880b8083395839`.
The first capture failed strict ASCII validation and was discarded. The fresh
`2026-09-04-layout-cleanup.txt` capture passes all 124 records, with zero timed
allocations, zero prepared first-frame allocations, matching host checksums and
full heap recovery. Single-operator fixtures retain 24 fewer heap bytes (9,596
to 9,572 at 200 pixels); the removed inline effect-workspace field is not part
of reported heap use. Temporal alternating/200 improves from 3,981 to 3,929 us;
operator reuse/200 is 2,195 us. DynamicArray/1600/16 layers regresses from
220,966 to 223,630 us (+1.2%). This is not a universal speedup claim; the exact
code-layout contribution to that small regression is not isolated. The same-ELF
repeat (`2026-09-04-layout-cleanup-repeat.txt`) also measures 223,630 us for that
case. Both captures pass all 124 records, zero timed and prepared first-frame
allocations, matching host checksums and full 163,840-byte heap recovery.

The full desktop Criterion comparison completed successfully: VM suite 483.98 us,
preparation 551.14 us, representative render 3.3271 ms, dense playback 8.2137 ms,
cold playback 8.1687 ms, controller output 8.7755 ms and temporal alternating/1600
230.21 us. Controller output is about 1% slower than the immediately preceding
8.6907 ms checkpoint, but about 33% less time than the original 13.17 ms baseline.
The direct VM suite is about 4% less time than the original approximately 503 us.
Do not attribute the controller improvement solely to interpreter instruction cost.

Dependency tracing additionally inspected libm 0.2.16 generic `scalbn`, `round`,
`floor`, `trunc`, `copysign` and `fabs`, plus their Float trait bit helpers.
For f32 the trait binds its bit storage to u32/i32; these paths preserve F and do
not convert to a wider float. Micromath 2.1.0 sine calls its F32 cosine, whose
floor converts through i32 and whose abs masks f32 bits. This closes the inspected
source helper chain beyond the public f32 API signatures. It is source evidence,
not an all-input trig accuracy guarantee or a general arbitrary-callback proof.

### Nested operator stack assessment (incomplete hardware coverage)

Elaboration computes `vm_workspace_count` from DSL graph depth and assigns each
operator a workspace slot. That reserves heap-backed VM registers; it does not
bound the Rust call stack. `sample_signal_pixel` calls a DSL interpreter whose
SignalSampler recursively enters `sample_signal_pixel` for upstream reads.

The current ELF above has these Xtensa `entry a1` frame sizes, inspected directly
with objdump: `sample_signal_pixel` 352 bytes, `run_operator_program` 128 bytes,
`Vm::run` 320 bytes, and `GraphSignalSampler::sample_signal` 112 bytes. These alone
sum to 912 bytes for a simultaneously active recursive DSL chain level; this is
not a complete worst-case stack bound, because other calls, root evaluation,
interrupts and register-window handling also matter. The linker stack interval
is `_stack_end_cpu0=0x3ffc8a10` to `_stack_start_cpu0=0x3ffe0000` (95,728 bytes),
not a measured free-stack figure and not a recommended task stack size.

Current board fixtures exercise one operator level, so they do not prove nested
operator stack safety. Next validation should add a bounded nested fixture with
host checksum agreement and measure stack use on the board before choosing a
depth budget or changing evaluation. No overflow has been reproduced, and no
new scheduler, continuation engine or platform-specific runtime has been added.

Remaining completion gates are nested stack/capacity coverage, the all-valid-path
allocation and malformed-prepared-data boundary review, controlled explanation
of persistent regressions, and a final consolidated requirement/LOC report.
Overall tracked source growth remains +1,441 nonblank lines, not net removal.

### Nested fixture implementation checkpoint

The shared profiling workload now extends the one-operator fixture into chains
of 2, 4 and 8 operators at each of the four pixel counts. Each chain shares one
operator bytecode program and assigns separate VM workspace slots by depth.
Host golden generation compares fresh and reused workspaces across all 32 sample
times. The capture verifier now requires 136 records (12 additional cases).
This changes profiling only, not the runtime implementation or evaluation model.

Full `pnpm check`, firmware release build and firmware release clippy pass.
Flashing completed successfully for ELF SHA256
`c3f9745f460eb95b48ddeb95fe0160e2599f3ce09ff34161f591888804fae321`.
The complete capture is saved as `2026-09-04-nested-operators.txt`: all 136 cases
match host checksums, all timed allocations are zero, prepared first-frame
allocations are zero, and the full 163,840-byte heap is recovered. At 200 pixels,
depths 2/4/8 take 4,317/8,549/17,003 us and retain 9,740/10,080/10,760 bytes.
At 1,600 pixels they take 34,403/68,230/135,887 us and retain
68,540/68,880/69,560 bytes. This is approximately linear execution cost per
operator, not allocation churn. The new cases measure time, retained heap,
allocations and checksums, not stack high-water use. No safe maximum depth is
asserted. The current build's HAL configuration does not enable
`soc_has_assist_debug` or `assist_debug_has_sp_monitor`; declarations under
`rustc-check-cfg` do not establish that the peripheral is available.

The collector now rejects nonzero timed allocations, prepared first-frame
allocations, missing first-frame records and unrecovered final heap directly.
An in-memory serial replay passes the saved valid capture and confirms rejection
of deliberately injected timed allocation, prepared allocation, heap-leak and
missing-first-frame cases. The device capture preceded this verifier edit and
was subsequently validated through the updated verifier by replay.

One concrete remaining runtime cost is that recursive `sample_operator_pixel`
uses `sample_operator`, which restarts at entry zero, while the outer frame
operator can reuse its uniform prefix across pixels. Reusing nested prefixes
would need to distinguish the operator/parameters and sample time actually held
in each shared VM slot, including sibling nodes and temporal revisits. Blindly
enabling reuse is incorrect. No such change is included in this checkpoint.

### Nested uniform-prefix reuse (validation in progress)

Each existing operator VM slot now holds its workspace plus the last successful
`(node_index, SampleTime)` identity. Recursive sampling reuses the compiler's
existing pixel entry only when this identity matches. Every frame clears the
identities, errors invalidate the sampled slot, and sibling nodes or changed
times restart initialization. No second interpreter or per-node register bank
is introduced. The identity occupies additional retained storage per depth slot;
allocation and timing effects await the updated board capture.

The five uniform tests pass, including depth eight versus full-prefix execution
and a new sibling/shared-slot case with different bound gains and temporal
revisits. Existing seven allocation tests passed after the runtime change.
Depth-eight repeated dimming can correctly quantize to black, so nonblack
assertions remain for shallow depths while exact reference-output comparisons
remain for all depths. Firmware release build/clippy pass; full repository
checks are running. The initial flash failed with a bootloader data-checksum
error; a retry is in progress. No new board performance result is claimed.
Tracked nonblank source is 16,583 (+21 this runtime step; +1,495 original).

### Native operator VM displacement fix

A focused two-pixel graph (DSL identity -> native Invert -> DSL identity) proved
four allocations in prepared evaluation. Both frame and recursive dispatch took
a VM depth slot even for native operators. The native operator held the reserved
register storage while its upstream DSL operator sampled through the now-empty
slot. Native nodes now leave operator VM storage untouched; their unused local
default workspace contains no allocated storage. Both dispatch sites use the
same conditional take/restore behavior, without another runtime abstraction.

The regression now reports zero allocations, retains exact inverted output,
and passes backward/forward seeks. All eight allocation and five uniform tests
pass. Full checks passed for the preceding prefix-only change; full checks and
firmware build/clippy are running for this additional fix. No board result for
either change is available yet: two stub flashes failed data checksums, and a
57,600-baud ROM-loader attempt failed at FlashEnd. These are not successful
flashes and must not be represented as verified current board application state.

### Combined nested-prefix/native-storage validation

Full repository checks and firmware release build/clippy pass for both fixes.
The rebuilt ELF has SHA256
`92e5f4c54130189deaa6f12b71ec25b7f4bb6a8005aa6ab6b4a48307a05a150c`.
After the earlier failures, flashing with the normal stub at 57,600 baud
completed successfully. The updated 136-case capture is running, as is the
full laptop Criterion comparison; neither is a completed performance result yet.
The first capture stopped on a corrupt serial line and was discarded; a fresh
same-image capture has started. No corrupted bytes were stripped or accepted.
The second capture also failed strict ASCII validation, near nested8/1600;
a third same-image capture is running. COM4 remains present as the CP210x bridge
with Windows status OK, which does not establish physical-link reliability.
Valid rows before that failure give provisional mixed timing: nested4/800
34,125 -> 36,922 us, nested8/800 67,951 -> 61,835 us. These are not a completed
capture or a universal speedup claim; the different-depth regression still
requires investigation after a clean run. Corrupted records are not used.
The new mixed native/DSL allocation regression is currently host-tested only;
the board's existing nested cases contain DSL operators, not that mixed graph.

A direct-call/jump disassembly scan found no references to the common software
double add/subtract/multiply/divide or float-double conversion helpers in this
ELF. This complements the earlier source tracing, not an arbitrary-callback proof.
Tracked source is 16,597 nonblank lines (+14 for native conditional take/restore,
+1,509 versus original). Standalone regression tests are outside that count.

### Clean nested-prefix board capture and next controlled comparison

The third capture completed and is saved as `2026-09-04-nested-prefix.txt`.
All 136 records pass the stricter collector: zero timed allocations, zero
prepared first-frame allocations, matching host checksums, and full heap
recovery. At 1,600 pixels, nested4 takes 73,818 us versus 68,230 before (+8.2%),
while nested8 takes 123,626 us versus 135,887 before (-9.0%). Depth tags add
12 bytes per VM slot on this target (48/96 bytes for those two cases).
DynamicArray/1600/16 is 223,101 us versus 223,630 at the preceding checkpoint.
These combined-build comparisons do not isolate instruction savings from
placement/cache effects or the native-workspace change.

The profiling sources now additionally contain `nested4_full` and `nested8_full`,
which zero the existing program pixel entry, disabling prefix skipping without
changing the evaluator binary. Host golden generation compares both modes.
The collector expects 144 records for this next build. It is not built or flashed
yet: the desktop comparison is still running and no heavy build is started
alongside its measurements. This control isolates skipping within the current
tagged runtime, not the cost of tag tracking versus the earlier untagged runtime.

### Completed desktop nested-prefix/native-storage comparison

The full Criterion comparison completed successfully: direct VM suite 490.18 us,
preparation 566.24 us, representative render 3.3759 ms, dense playback 8.3744 ms,
cold playback 8.3235 ms and controller output 8.7411 ms. Operator reuse/1600
is 145.29 us; temporal grouped/alternating at 1600 are 229.78/233.84 us.
Compared with the immediately preceding layout-cleanup checkpoint, controller
output is nearly unchanged (8.7755 ms previously), direct VM is about 1.3%
slower (483.98 us), and temporal alternating about 1.6% slower (230.21 us).
These small point-estimate differences are not isolated causal measurements.
The saved Criterion baseline for temporal cases predates compiler read reuse,
so its reported approximately 45% gain is not caused by this latest step alone.

The same-image firmware control build started only after the full desktop
comparison exited. Its build, flash and capture remain pending.

### Same-image nested control validation started

Full repository checks and firmware release build/clippy pass for the added
control cases. Flashing at 57,600 baud completed successfully, with ELF SHA256
`feacaae47fc1d4e2cd01319b02b1caea752e39cb1fe53f9a82a1d10188267e05`.
The 144-case capture is running. In this ELF, `sample_signal_pixel` is 0x1e76
bytes (7,798), at flash address 0x4010ece0; `Vm::run` is 0x6fcf bytes in IRAM
at 0x40081c24. This identifies placement, not measured cache misses or a proven
cause of the earlier depth-dependent regression. Runtime source is unchanged
from the previous capture; only profiling/control coverage changed.

### Same-image nested control result

`2026-09-04-nested-control.txt` completed all 144 cases with matching host
checksums, zero timed and prepared-first-frame allocations, and full heap
recovery. Within the same ELF, four-level reuse at 1,600 pixels takes 54,878 us
versus 74,459 us with full prefixes (26.3% less time). At 800 pixels eight-level
reuse takes 52,355 us versus 71,927 us (27.2% less time). The 200/400-pixel
pairs show the same approximately 26-27% benefit and identical retained heap.

The previous four-level regression is not reproduced by disabling/enabling
prefix skipping within this binary. Further, the unchanged runtime source now
runs nested4/1600 at 54,878 us versus 73,818 in the previous firmware image;
only profiling control coverage changed between those builds. This is strong
evidence of sensitivity to executable/data arrangement, not a measured hardware
cache-miss attribution. The control retains tag tracking in both modes, so it
does not isolate that bookkeeping's cost. Keep the optimization and the control
cases; reverting on the earlier between-build number would discard a measured
same-image improvement. No additional placement trick was applied in this step.

### Allocation-capacity audit: empty automation windows

Source review followed the remaining growth sites in sequence evaluation, VM
registers/arrays, prepared curves, patch values and fixture controls. Patch
fixture-state `clone_from` reuses function-vector capacity reserved from profile
function counts; show explicit-control storage reserves the sum of fixture
addresses; filter/fixture output capacity comes from prepared patch layouts.
These contracts concern correctly prepared data, not arbitrary mutation of the
public prepared structures or standalone convenience APIs.

A concrete missed edge case was reproduced: an empty source/parameter curve
with curve automation produced three allocations on the first prepared frame.
`curve_window_into` emits one fallback point for an empty window, but
`reserve_window_capacity(0)` reserved no points, segments or crossing entries.
Preparation now reserves `point_count.max(1)` for those existing buffers.
Sampling semantics are unchanged. The new test verifies nonblack fallback
output and zero allocations; all nine allocation tests pass. Full repository
checks and firmware release build/clippy are running. This newest change is
not yet flashed, and the empty-curve and mixed-native regressions are still
host-only cases pending dedicated board coverage.

### Empty-curve board fixture preparation

Full checks passed for the two-line empty-window capacity fix. The shared native
automation fixture now accepts an empty-curve variant, and host golden generation
compares fresh/reused workspaces for both variants. New `empty_automation` cases
at 200/400/800/1600 pixels raise the next capture count to 148. The empty variant
uses a 0.5 minimum so the fallback output is nonblack rather than a vacuous black
checksum. Firmware build/clippy pass for this coverage; full checks are running
after the shared fixture signature/caller changes. This image is not yet flashed.
Mixed native/DSL board coverage remains to be added before the next capture.

### Shared mixed native/DSL board coverage

The host allocation regression and firmware now share the setup that inserts
native Invert between two DSL identity operators. Host golden generation checks
every output pixel against the independently inverted direct effect sample,
not merely another execution of the same graph. New `mixed_native` cases join
`empty_automation`, bringing the next collector requirement to 152 records.
Duplicate graph-construction code was removed from the standalone regression.

Firmware release build/clippy pass for these fixtures. Full checks passed for
the preceding empty-curve fixture and are running again after the mixed-fixture
changes. Flashing of the combined image is in progress; neither new edge case
has a completed hardware capture yet.

### Combined allocation-edge validation checkpoint

Full repository checks and firmware release build/clippy pass with both new
shared fixtures. ELF SHA256 is
`8bbd4685965d573823eec1f27f348b238b84f625e62683cef91adf04a7936cd8`.
The initial flash failed a data checksum; an unchanged-image retry is running.
A fresh laptop comparison has started for the current runtime, whose only
change since the last completed comparison is the preparation-time one-point
minimum reservation. No new board or desktop result is claimed yet.
Tracked source is 16,653 nonblank lines (+1,565 original); this includes moving
the mixed-graph fixture out of an uncounted standalone test into shared firmware
source. The count is not a whole-repository net diff.

### Completed combined allocation-edge board validation

The unchanged-image retry flashed successfully. The first 152-case capture
completed without serial corruption and is saved as
`2026-09-04-allocation-edges.txt` for ELF `8bbd4685...a7936cd8` above.
Every timed sample and prepared first frame is allocation-free, all host
checksums match, and all 163,840 configured heap bytes are recovered at exit.

Empty automation at 200/400/800/1600 pixels takes 208/402/789/1,564 us and retains
9,640/18,040/34,840/68,440 bytes. The mixed native/DSL graph takes
3,755/7,499/14,986/29,961 us and retains 9,688/18,088/34,888/68,488 bytes.
These newly introduced board cases establish current correctness/allocation
behavior, not pre-fix hardware speedup; the pre-fix allocation reproductions
were on the host. DynamicArray/1600/16 measures 223,096 us, essentially matching
223,097 in the preceding controlled image. Full laptop comparison is ongoing.

### General array-storage coverage gap and prepared fixture

The completed board DynamicArray fixture uses register selection after lowering,
so it does not exercise `ArrayStorage`. The existing host
`array-lifetimes.effect.dawn` fixture is now also included in firmware generation,
with an assertion that compiled `array_capacity` remains nonzero. It exercises
nested arrays, aliases, reassignment and loop construction with four iterations.
Only direct VM and single-layer prepared cases are added, not 4/16-layer stress
that could exceed the board heap. Counts remain 200/400/800/1600, adding eight
records (next collector total 160). Host generation compares prepared bytes
against direct sampling as for the other fixtures.

This is added profiling coverage, not another runtime change. The fixture has
not yet been built or flashed; heavy builds remain deferred until the current
laptop comparison finishes. The last completed board evidence remains 152 cases.

### Completed current-runtime desktop comparison

The full comparison after the one-point reservation fix completed successfully:
VM suite 489.79 us, preparation 556.08 us, representative render 3.4743 ms,
dense playback 8.4337 ms, cold playback 8.4395 ms, controller output 8.9356 ms,
and temporal grouped/alternating/1600 229.49/232.80 us. Versus the previous
completed run, controller output is about 2.2% slower (8.7411 ms), while VM
(490.18 us) and temporal alternating (233.84 us) are essentially unchanged.
The only intervening runtime edit sets a preparation-time minimum reservation;
these point estimates do not prove it caused the controller timing difference.
Controller output remains about 32% less time than the original 13.17 ms batch.

The general array-storage firmware build started after the comparison exited.
Its build and hardware checks are pending; runtime source did not change for it.

### General array-storage firmware validation started

Full repository checks and firmware release build/clippy pass for the reused
array-lifetime fixture. Flashing at 57,600 baud completed successfully for ELF
SHA256 `e89288566a3cf482d288cef6864356dea40c6e04aaf3874ed6e1ba08f68ce5fa`.
The 160-case capture has started; no completed array-storage board result is
claimed yet. The latest full desktop result above uses the identical runtime
source; subsequent changes only extend profiling coverage.

### Completed general array-storage board capture

`2026-09-04-array-storage.txt` passes all 160 records, matching host checksums,
zero timed allocations, zero prepared first-frame allocations, and complete
163,840-byte heap recovery. Generated ArrayLifetimes bytecode retains a 30-slot
arena with width three, verified in generated source and guarded by the build's
nonzero-capacity assertion. Thus the capture exercises general nested-array
storage rather than only scalarized syntax.

At 400/800/1600 pixels, direct VM batches take 41,626/83,242/166,476 us, while
prepared frames take 508/896/1,671 us and retain 24,288/41,088/74,688 bytes.
The effect does not depend on pixel context, so prepared evaluation executes its
array operations once and broadcasts the result; direct VM intentionally
executes them separately for every pixel. This is not a general 100x interpreter
speedup. Direct VM first-use initialization allocates 12 times, as expected;
prepared workspace creation performs that allocation before the first frame.

## LOC accounting

A pre-edit snapshot counted 15,088 nonblank Rust lines across the 52 files under runtime/src, language/src/dsl, elaboration/src, and firmware/dawn-profile/src. At the shared-Identifier checkpoint this same set contains 15,101 (+13), including the new layered firmware fixture and in-module tests. This is not whole-repository net LOC: standalone tests, benchmark fixtures, build script, linker config and documentation are outside that count. VM override machinery and identifier reservation code were removed; the overall request is not yet a demonstrated net-removal refactor.

The calculated-array checkpoint contains 15,312 lines in that same set (+224
versus the original snapshot, +211 versus shared identifiers). The increase
buys compile-time storage bounds, reusable slots, alias ownership and generator
export handling. This is a real complexity cost, not a net simplification claim;
compile-time scalarization should remove work for common fixed-index arrays.

After fixed-index lowering the same directories contain 15,579 nonblank Rust
lines in 53 files (+267 this step; +491 from the original snapshot). All this
step's implementation growth is host-side compiler analysis. Runtime code did
not grow. Tests, benchmark fixtures and docs remain outside this count. This is
still not the requested net-removal outcome.

After generic cleanup the tracked directories contain 15,881 nonblank Rust
lines in 54 files (+302 this step, +793 from the original snapshot). The old
array-specific removal and reference-input scan were removed, but the generic
typed-operand visitor and compaction add more compiler code than they remove.
Runtime code again did not grow. This is a code-size tradeoff for smaller
programs and runtime storage, not a claim of net source simplification.

The call-site guard adds three nonblank runtime lines, bringing this directory
count to 15,884 (+796 versus the original snapshot). No new runtime abstraction
was added.

The color-buffer change replaces one executable statement with a no-op match
arm and adds two explanatory comments: 15,886 nonblank Rust lines in the same
54 source files (+798 versus the original snapshot). The new integration test
is outside that historical count. No new runtime data structure was added.

After RGB lookup support, the same 54 source files contain 16,143 nonblank Rust
lines (+257 this step; +1,055 versus the original snapshot). This includes the
in-module exhaustive tests and firmware workloads, but excludes the standalone
benchmarks, build script and docs. The implementation extends the existing
packing pass and filter variant; it removes prepared/runtime work, not source
lines overall. The broader net-simplification goal remains unmet.

Uniform-prefix support brings the same 54-file count to 16,286 nonblank Rust
lines (+143 this step; +1,198 versus the original snapshot). Most new logic is
compiler dependency analysis; the runtime adds an instruction offset and local
reuse flag. Standalone regression tests, build-script changes, linker placement
and benchmark artifacts remain outside this historical count.

Fixed-array indexed selection brings the count to 16,324 nonblank Rust lines
in the same 54 files (+38 this step, +1,236 from the original snapshot).
This adds one bytecode operation and extends existing lowering/operand handling;
it eliminates runtime array work for eligible programs, not source lines overall.

Operator prefix reuse and correction of the obsolete workspace-allocation
comment bring the count to 16,358 nonblank Rust lines (+34 this step, +1,270
from the original snapshot). The standalone integration test is excluded.

Shared operator profiling fixtures bring this count to 16,404 (+46 this step,
+1,316 from the original snapshot). This step changes profiling code, not the
runtime implementation. Host benchmark/build-script code remains excluded.

Node-indexed signal caching reduces the same count to 16,391 (-13 this step,
+1,303 overall). It removes the key type and cache-budget handling. The new
standalone allocation regression test is excluded from this historical count.

Workspace initialization reuse brings the count to 16,396 (+5 this step,
+1,308 overall). It extends the existing constructor with the instruction entry
and removes the two subsequent instruction-pointer assignments.

Equal-target routing and recursive context-parameter validation bring the
same count to 16,406 (+10 this step, +1,318 overall); standalone tests are excluded.

Native automation release ordering adds one ownership comment, bringing the
count to 16,407 (+1,319 overall). The regression test is outside this count.

Shared native-automation firmware coverage brings the tracked count to 16,478
(+71 this step, +1,390 overall). This is profiling setup, not added runtime logic;
the corresponding deletion of duplicate standalone test setup is excluded from
this historical count, as are host golden-generation additions.
