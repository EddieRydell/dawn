# Runtime optimization second pass

## Scope and gates

Keep one stateless runtime, with general compiler/elaboration solutions rather
than runtime effect categories. Preserve arbitrary-time evaluation, allocation-free
successful prepared frames, output checksums and bounded workspace ownership.

Targets, in order of investigation:

1. Upstream effect preparation and uniform reuse during operator sampling.
2. Safe uniform resource/color work and time-independent pixel calculations.
3. Repeated same-time graph traversal and per-pixel interpreter overhead.
4. Redundant color/output passes where prepared routing proves they are unnecessary.

Coverage must include stacked chases, fades/pulses and mark-based effects, not
just the existing complex effects or identical synthetic layers. Add realistic
mixed/layered fixtures before changing their runtime paths. Tests are authorized.

## Measurement protocol

The first pass's final source and saved 160-case capture are the starting point;
see `runtime_optimization_2026-09-04.md`. Starting tracked source count is 16,653
nonblank Rust lines in 54 files under the same four directories documented there.

Before runtime changes, run `pnpm bench:effect-vm:save` and repeat the installed
firmware capture. This deliberately starts a new Criterion `effect-vm` baseline;
the first pass's results remain recorded separately. New fixtures need their own
unoptimized baseline. Do not compare quick-pass estimates with settled full runs.

Investigate regressions using repeated uncontended runs, identical workload and
output checks, and same-image controls where possible. Do not label unexplained
differences noise or revert useful structural changes solely for one slower result.
Track firmware hashes: code placement has previously changed timings materially.

## Attribution profiling investigation

Current board connection is a CP210x UART bridge on COM4. Firmware uses esp-hal,
not ESP-IDF/FreeRTOS. IDF SystemView/app-trace integration is therefore not a
drop-in profiling solution for this program.

The installed esp-hal 1.1.2 Xtensa interrupt dispatcher passes the saved
`xtensa_lx_rt::exception::Context` to peripheral handlers. Investigate periodic
interrupted-PC sampling using this existing HAL mechanism, storing samples in
bounded firmware-owned memory and transmitting only outside measured windows.
Resolve addresses against the exact ELF on the laptop. This should initially be
described as a statistical function profile, not a full call-stack flame graph.
Reliable stack reconstruction, interrupt masking bias and sampling overhead must
be established rather than assumed. No profiling hooks belong in the shared
runtime merely to make the firmware harness convenient.

Status: baseline processes started; profiler feasibility is not yet verified on
hardware. No runtime implementation changes have been made in this pass.

The first repeated board capture was rejected near ArrayLifetimes for corrupt
non-ASCII serial bytes; no result from that partial capture is accepted as a
completed baseline. A fresh capture is running. The original complete saved
capture remains available.

An initial `pc_profile` firmware binary now uses the saved interrupt PC and a
4096-entry static sample buffer. It is gated by `pc-profile` (enabling the existing
HAL's unstable timer API), so the normal benchmark binary does not include it.
Two-second windows run without sampling, at 997/1999 microsecond sampling periods,
and without sampling again. Serial output is outside those windows. This is an
unvalidated implementation pending build/hardware checks, not measured profiling
evidence. Full-stack unwinding is deliberately not claimed. Laptop baseline timing
is allowed to finish before any firmware compilation competes for CPU resources.

The second repeated board capture also failed strict ASCII decoding and was
discarded. The serial corruption is not evidence of a runtime timing regression;
its physical/transport cause has not been established.

`capture_pc.py` now validates complete profiling/control windows and symbolizes
interrupted addresses with the installed Xtensa `nm` against a SHA-256-identified
ELF. Unresolved addresses remain explicitly unresolved. Four collector tests pass
(`uvx --from esptool python -m unittest test_capture_pc`): complete symbolization,
missing samples, corrupt serial bytes, and incomplete fixture coverage. These are
host parser tests, not evidence that on-device sampling works. Hardware build and
validation remain pending while the full desktop baseline continues.

## Baseline completion and profiling dependency gate

The full `pnpm bench:effect-vm:save` process completed successfully. The new VM
baseline estimate is 467.43 us; representative rendering 3.2424 ms and dense
60-frame playback 8.1688 ms. These precede any runtime edits in this pass.

The first profiler build with `--locked` stopped before compilation because
`esp-hal/unstable` changes dependency resolution. Source inspection confirms
that the timer module is private without that feature, and the feature activates
additional optional HAL dependencies (digest, Embassy and embedded I/O/CAN/RNG
traits). Approval was requested before changing the lockfile. No new dependency
has been resolved or installed for this feature, and no successful profiler
build or hardware sampling claim is made. The broader runtime/fixture work is
not blocked by this profiling-only approval gate.

Each profiling window now checks its last produced frame against the existing
host-generated checksum after disabling sampling, outside the timing interval.
This protects an output actually calculated under interrupts, rather than only
checking a later replay with interrupts disabled. It does not check every frame
in that window, and must not be described as such.

## Approved profiler build and mixed native coverage

The user approved the profiling-only HAL dependencies. The firmware lockfile
resolved 14 additional packages; the runtime manifest is unchanged. The sampler
now builds with `--release --locked --features pc-profile --bin pc_profile` and
passes the equivalent release clippy command with warnings denied.

The installed HAL's handler macro rejects a context-bearing function despite its
Xtensa dispatcher passing that context. The firmware uses the dispatcher's actual
signature with a documented function-pointer erasure at handler registration.
It also supplies the existing allocator hooks and asserts no allocation occurred
in each profiling window. No shared-runtime instrumentation is introduced.

`chase_pulse_show` adds shared profiling setup with alternating native chases and
pulses, varied colors, starts, durations, chase widths and direction. Criterion
now covers 1/4/16 layers and checks fresh/reused workspaces across forward/backward
times, with nonblack output assertions. Mark-generated effects and operator mixes
are still required; this fixture does not substitute for that coverage.

Full `pnpm check` passed after adding the fixture and benchmark. The focused
`prepared_chase_pulse` baseline is running under the `second-pass` name. The first
sampler flash is in progress; hardware sampling is not yet verified.

The first sampler flash failed with a FlashDeflData communication timeout; a
retry is in progress. This is not a successful installation or profiling result.
The focused desktop fixture reports 6.4508/14.378/56.254 us for 1/4/16 layers
at 200 pixels, before runtime changes; these are desktop, not ESP32 timings.

## First complete on-device statistical profile

The retry flash completed and `capture_pc.py` accepted all 36 windows (nine
effects, two sampling periods and before/after disabled controls). The tool-output
transport truncated some raw PC batches, so its reconstructed artifact was removed;
it is not a complete saved capture. The collector now supports exclusive-create
raw output directly to disk and emits only case/symbol summaries to the console.
A repeat is required for a complete inspectable raw artifact.
ELF: `012090c79be7449c3566e31df909283c210c6fb3656c2a84a36c53da2a049ab8`.
No allocation assertion, last-frame checksum assertion or capture validation failed.

This establishes working interrupted-PC attribution, not full-stack flame graphs.
At 997 us sampling, the complex workloads show roughly 0.3-0.4% throughput
overhead versus their same-image disabled controls. Software **f32** division
(`compiler_builtins::float::div::__divsf3`) and VM dispatch dominate those samples;
the indexed-array fixture instead concentrates in VM execution. This is distinct
from the software-double problem removed in the first pass.

Important unresolved discrepancy: disabled-control ScanSweep takes about 12.65 ms
per frame in this profiler image versus 5.249 ms in the normal harness. ImpactBurst
is about 16.94 ms versus 5.447 ms. The shared runtime source is unchanged; the
images differ in linked code/layout, enabled HAL features, harness and sampling
storage. Low ISR overhead does not explain that between-image gap. Do not apply
these profile percentages or absolute timings to the normal image without further
controlled investigation. Both images contain the same-sized 682-byte division
implementation at different flash addresses. Profile IRAM text is 51,400 bytes
versus 53,472 in the normal image; BSS is 114,712 versus 98,336 bytes.

The collector reports only sized ELF text symbols. ROM routines without sizes
remain unresolved rather than being guessed from neighboring addresses. Sampling
excludes UART output but includes the small harness time-check loop; periodic
aliasing, interrupt masking and leaf-only attribution remain limitations.

### Division placement control in progress

The original profiler ELF was preserved as
`firmware/dawn-profile/target/pc-profile-flash-division.elf` with the hash above.
The linker hook now places the f32 division implementation and wrapper, including
their literals, in IRAM. This is a board-placement experiment, not altered math
or a new runtime path. The rebuilt image passes release compilation and resolves
the 682-byte implementation at `0x4008ccdc` and wrapper at `0x4008cf88`, rather
than flash. New ELF hash:
`472dede0a3d37fd5218fbee9944f507c864f3d34d390393abaf077942cbe3264`.
It has not yet been flashed; the direct-to-disk original-image capture is still
running. No performance benefit from this control is claimed yet.

The original-image repeat subsequently completed successfully. The directly saved
`firmware/dawn-profile/results/2026-09-05-pc-repeat.txt` contains all 36 validated
case windows, 27,110 raw interrupted PCs, and the final `DAWN PC END` marker.
The IRAM-division control is now flashing; it is not yet a measured result.

### Refined upstream-automation diagnosis

Current `prepare_effect_params` already checks `workspace.sample_time`, and
`prepare_native_sample` retains the prepared native sample at that time. Calling
those helpers per upstream pixel is repeated dispatch/lookup, not necessarily
repeated curve evaluation or native preparation. Preserve that existing cache.
In contrast, recursive `sample_signal_pixel` applies operator automation bindings
on each visit without an equivalent time check. Upstream effect timing and
uniform-prefix execution are still repeated. The optimization must target these
actual distinctions rather than add a second cache for work already cached.

The division-placement retry flashed successfully and its raw capture is running
at `results/2026-09-05-pc-iram-division.txt`. The first disabled ScanSweep window
is 2,002,719 us / 390 frames = 5.135 ms per frame, versus 12.655 ms in the original
profiler image. This is a partial-run observation, not yet a completed comparison.

The first `prepared_marks` benchmark attempt failed its explicit assertion that
the starter's first sequence contains native mark children. Current starter root
sequence zero is `empty`, and the layer-test mark collection is empty too. Existing
native parity coverage therefore cannot stand in for a real mark-workload baseline.
The new benchmark currently exposes this missing fixture rather than silently
timing an empty sequence; constructing explicit nonempty mark inputs is the next
coverage task. No mark timing is accepted and the benchmark is not yet passing.

The IRAM-division capture completed successfully with all 36 windows and final
marker saved. Disabled-control mean frame times (first control, microseconds):

| Effect | Flash division profiler | IRAM division profiler |
| --- | ---: | ---: |
| ScanSweep | 12,655 | 5,135 |
| ImpactBurst | 16,944 | 5,282 |
| SparkleComet | 22,383 | 8,151 |
| ShimmerField | 12,763 | 5,172 |
| PixelRamp | 774 | 774 |
| DynamicArray | 1,836 | 1,836 |

Each image's final disabled control closely reproduces its initial control.
The placement change restores the complex cases near normal-harness timing,
while the low-division controls stay effectively unchanged. This is controlled
evidence of a large division/literal placement cost, not a 2-3x improvement over
the previous goal's normal runtime baseline. Flash/data-cache counter attribution
is still unavailable; no arithmetic or shared-runtime source changed.

## Explicit nonempty mark fixture

The mark benchmark now clones the starter layer-test project in memory and
supplies 32 marks at 50 ms intervals, with 1.2-second MarkPulse/MarkChase children.
It retains the existing target/layout and gradient, supplies required curve and
generator inputs, and removes unrelated effect/automation/control instances from
that in-memory sequence. Authoring files are not edited. Each variant asserts at
least 32 prepared children, the intended native child implementation, nonblack
output, and fresh/reused workspace equality across forward/backward sample times.

The focused baseline now compiles and passes fixture setup; timing is running.
This is real generator expansion through elaboration with starter geometry, not
a claim of 200-pixel device coverage. Device mark fixtures, layered operator mixes,
and first-frame allocation tests for these workloads remain required.

The focused Criterion run completed successfully: prepared mark-pulse playback
557.44 us and mark-chase playback 4.4806 ms per sampled frame on the laptop.
These are new pre-optimization baselines, not gains, and include the actual
overlapping generated children in the project layout.

## Mixed native effects on the PC profiler

The shared 200-pixel chase/pulse fixture is now included at 1/4/16 layers in the
device PC profiler. Host build-time generation checks every golden frame against
a fresh workspace and rejects black-only output. Device windows use these exact
goldens for their last-frame checks and the existing allocation assertions.
The collector now requires 48 windows across 12 cases; its four tests pass with
the expanded contract. Earlier 36-window files remain historical captures of the
nine-case firmware, not inputs accepted by the expanded live collector.

Both firmware binaries pass release clippy with warnings denied, and the expanded
profiler builds successfully. Flashing and refreshed full repository checks are
in progress. No device timing for the new fixture is claimed yet.

The flash and full repository checks completed successfully. The first expanded
capture failed strict ASCII decoding and its partial file is not an accepted
result; a repeat is running. No failed capture is repaired by dropping bytes.

A shared 200-pixel native mark fixture now generates explicit mark children at
setup using the existing native generator implementation. Identical full-target
children reuse the original target geometry; subset children retain their exact
pixel context. The desktop benchmark exercises fresh/reused workspaces and
nonblack output for this device-oriented fixture before firmware integration.
The existing project-elaboration mark benchmark remains separate coverage of
the actual authoring/elaboration path. No runtime optimization has been applied
yet; these establish required nonempty mark baselines.

The focused 200-pixel mark fixture baseline completed: pulse 14.078 us, chase
236.66 us on the laptop. Host build-time golden generation now covers all 32
frames for both fixtures and checks fresh-workspace parity and nonblack output.
The device profiler includes these cases; the live collector now requires 56
windows across 14 cases. Four collector tests pass. Both firmware binaries pass
release clippy and the expanded profiler builds; its flash is in progress.

The preceding 12-case chase/pulse repeat completed successfully and is saved as
`results/2026-09-05-pc-chase-pulse-repeat.txt`. Its 16-layer disabled control takes
about 14.0 ms/frame at 200 pixels. F32 division remains a major sampled cost in
this native workload even with the routine in IRAM. This is an actual device
baseline, not the much faster desktop result. Current runtime source is still
unchanged; the next runtime work can be evaluated against these baselines.

## First runtime change: upstream uniform-prefix reuse

The 14-case mark firmware flashed successfully; baseline capture is running
against the preserved `target/pc-mark-baseline.elf` (hash
`c18d0e604836a3529779202ca0e548124aac2354621328bce6664d6777fc8520`).
That binary predates the following runtime change.

`EvaluationWorkspace` now records the effect/time identity held by its existing
shared effect VM. Recursive upstream sampling reuses numeric prefix registers
only when this identity matches. Frame entry and direct layer traversal clear
the identity; recursive evaluation invalidates it before running and restores
it only on success. No additional VM, heap allocation, or effect classification
is introduced. Multiple interleaved effects/times can still force reexecution;
this small change does not claim to solve all upstream preparation work.

Nine allocation tests and all six uniform-sampling tests pass. The new regression
compares against bytecode with prefix reuse disabled across single/multiple effects,
different effect timings, temporal reads and backward seeks. Full repository
checks are running. No laptop/device speedup is claimed until measured.

The full repository check passed for that prefix-only change. A focused Criterion
comparison against the saved `effect-vm` baseline subsequently measured operator
reuse at 18.486 us/200 pixels and 144.79 us/1600 pixels. Temporal sampling regressed:
grouped 30.963 us/200 (+6.92%) and 245.86 us/1600 (+6.85%); alternating 31.300 us/200
(+7.17%) and 247.02 us/1600 (+5.77%). The consistent direction across sizes is not
being dismissed as noise, regardless of Criterion's configured advisory threshold.
These fixtures use a small PixelRamp source and change upstream time within each
pixel, so the new last-use bookkeeping cannot amortize across pixels. This is a
mechanistic explanation to validate with controlled follow-up measurements, not
proof that every observed percentage comes from that bookkeeping. The patch stays
in place while the larger upstream simplification proceeds.

The 14-case baseline captures `pc-mark-baseline`, `pc-mark-baseline-repeat`, and
`pc-mark-baseline-repeat2` all failed strict ASCII decoding (0xff); their partial
files are NOT accepted benchmark/profile results. The preserved ELF remains
available. Earlier complete 9/12-case captures are still valid for their own
coverage; there is no accepted on-device mark measurement yet.

## Operator automation reuse

The existing per-node operator automation storage now also holds its last sample
time. Recursive sampling applies bindings only when that time changes. Frame entry
invalidates these times, preserving edits at an unchanged playback time; failed
updates invalidate before mutation. This adds no successful-frame allocations and
no per-effect classification. It does add one optional timestamp per allocated
operator automation slot and a frame-entry invalidation pass.

The new regression compares recursive same-time and two-time reads with independent
direct frame evaluation, across backward seeks and automation mapping edits. Seven
uniform tests and nine allocation tests passed. The upstream prefix regression now
also includes an identity operator to exercise positive reuse, not only alternating
times. Full checks for this second change are running; its performance has not yet
been measured separately.

Full `pnpm check` and both ESP32 firmware binaries' release clippy checks passed
after the automation change and expanded identity-operator regression. No updated
runtime image has been flashed yet; the device still has the preserved mark baseline.

## Uniform color expressions and integer chase sections

The compiler's existing uniform-prefix pass now includes pure scalar color math
(RGB/HSV construction, mix, scale, inversion, intensity and binary color operations)
and float mixing. Eligibility still requires single-assignment destinations and
uniform operands. Reference-producing and potentially failing resource sampling
remain in the pixel body; this is not a speculative resource-hoisting change.
No VM state or runtime dispatch was added. Mixed uniform/pixel HSV and color-mixing
tests, including a mutable branch-local color, match independent scalar sampling.
The full repository check passed for the compiler-only change.

Native chase/spin section count and section index now use 32-bit integer division
instead of converting integer geometry to float, dividing, and flooring. Normalized
positions and effect math remain f32. This replaces two software float divisions
per native sample without adding prepared storage. A new reference parity test
covers Chase/Spin, widths -1/0/1/3/7/65535, three gradient modes, both directions,
pixel counts 1/2/200/65535, and multiple positions/times. It passes exactly against
the existing DSL reference. This validates tested fixture domains, not all possible
i32 contexts (the integer calculation also avoids the old large-float rounding).

The updated profiler built successfully and is preserved at
`target/pc-color-integer-chase.elf`, SHA256
`d3ef70a90bcddffbd3074190526df5d8b259f95b23d4a79ad4b68bf048f7d047`.
It has not been flashed yet. Another preserved-baseline capture is running, and
the full repository check for the combined changes is running. Performance remains
unmeasured for these two changes at this checkpoint.

Full repository checks and release clippy for both firmware binaries passed.
The laptop chase/pulse Criterion comparison completed: 1/4/16-layer estimates
6.2564/13.962/55.737 us. Reported mean changes versus the saved baseline were
-1.26%/-1.65%/-0.69%; the 16-layer interval includes zero. These small changes
are not evidence of a substantial desktop speedup.

`results/2026-09-05-pc-mark-baseline-repeat3.txt` is a complete accepted 56-window,
14-case baseline capture, including allocation and last-rendered-frame checks.
Disabled controls measure MarkPulse200 at about 2.35 ms/frame and MarkChase200 at
about 54.9 ms/frame (37 frames/2.033 seconds). The latter's 997/1999-us profiles
attribute 46.68%/49.31% to software f32 division, with most remaining samples in
native sampling. This is a heavy overlapping mark fixture, not a universal
200-pixel frame-rate prediction. The updated image is now being flashed for
comparison; no updated on-device performance is claimed yet.

The updated image flashed and its complete accepted capture is
`results/2026-09-05-pc-color-integer-chase.txt`. Averaging each case's two disabled
controls gives chase/pulse 1/4/16-layer times of 1313.17/2827.57/11016.98 us versus
1689.50/3571.36/13998.60 us: reductions of 22.27%/20.83%/21.30%. Complex DSL fixtures
are +0.07%-0.12%; MarkPulse/MarkChase are +0.47%/+0.24%. UniformFade is +2.09% and
ArrayLifetimes +3.21%; these small but repeatable-looking image differences are
recorded, not automatically dismissed. Several unaffected cases gained roughly
six microseconds per frame, so there is also a fixed-cost/image-layout component
to investigate. Allocation and host-output assertions passed all 56 windows.

The desktop dense-controller benchmark regressed versus the saved baseline:
9.6387 ms on the first run (+11.71% mean), 9.3596 ms on repeat (+8.48%). Controlled
source A/B checks, restoring each optimization after measurement, found:

- without the new color-hoisting eligibility: 9.6511 ms (+11.86%);
- without the upstream effect identity field/bookkeeping: 9.2687 ms (+7.42%).

Neither removal eliminates the regression. Color hoisting and upstream identity
reuse have been restored. A control using the previous native floating section
calculation is now running; this is a temporary attribution experiment, not a
decision to discard integer geometry or the verified ESP32 improvement.

The floating-section control measured 9.4169 ms (+9.14%). A combined control with
all four second-pass runtime/compiler changes removed measured 9.4186 ms (+9.16%).
Also omitting the new benchmark entrypoints measured 9.2922 ms (+7.70%). Thus the
earlier saved desktop result does not reproduce even with the pre-change runtime
logic; these controls do not implicate the new optimizations as the source of the
whole historical gap. The remaining difference is NOT labeled random noise: suite
ordering/warmup, host state and build layout remain unisolated possibilities. The
same-session updated/control measurements overlap in practical range, but a full
suite comparison under the original invocation is still required. All temporary
controls have been removed and the optimizations/benchmark entrypoints restored.
The post-restoration full repository check is running.

The post-restoration full check passed. Current scoped source count is 17,126
nonblank Rust lines/55 files, versus 16,653/54 at this pass's start (+473 lines).
This includes the new firmware profiler and shared mixed/mark fixtures, but not
Python collector code, integration tests, benchmark files or documentation.

The original `pnpm bench:effect-vm:compare` workflow is now running. The seven new
mixed/mark fixtures had genuine pre-change data saved as `second-pass`; those
directories were copied unchanged to their previously absent `effect-vm` tags so
the full comparison can include them. No existing baseline was overwritten, and
the copied estimate hashes were verified equal. The original saved runtime cases
remain unchanged. Full-suite results are pending.

That full comparison completed successfully, before the next changes below. It
still measured desktop mean controller time 9.3158 ms and VM-suite slope 505.18 us;
the full invocation did not recover the earlier baseline. Chase/pulse 16-layer
slope was 53.677 us; device-oriented mark pulse/chase slopes 13.248/224.29 us.
The distinction between quick-pass output and final full-pass estimates matters:
the quick VM result was 467.54 us, not the final full result. Historical desktop
baseline drift remains an open measurement question, with same-session controls
documented above; it has not been attributed to any single optimization.

## Ordered uniform resource sampling and mark-chase arithmetic

The compiler now lifts uniform direct curve/gradient parameter samples and related
clamped/crossing/scaled operations into its existing scalar initialization prefix.
An ordered candidate cannot cross an earlier unlifted potentially failing operation
or control-flow boundary. Pure scalar expressions can still be lifted independently.
This uses the existing scalar registers, adds no runtime storage, and does not keep
resource references alive across VM invocations. Tests assert actual curve/gradient
instructions are lifted, compare output with independent samples across seeks, and
verify untaken gradient branches and earlier array-index errors are preserved.

MarkChaseChild now computes its shared normalized pulse span once and uses integer
remainder for section position, only in the gradient mode that consumes it. This
removes redundant division and floor work without new prepared data or execution
categories. A new native-versus-DSL reference test covers schedule, targets and
sample output for four section widths, three gradient modes, two marks and multiple
pixels/times. All nine uniform, nine allocation, and three native parity tests pass.
Full repository checks and the updated ESP32 image build are running.

Those checks passed. The new profiler image is preserved at
`target/pc-resource-mark.elf`, SHA256
`8b5a090da054747a85be483d4f2714499366d4dbea0045f192181d3578ae9ad8`.
Its accepted complete capture is `results/2026-09-05-pc-resource-mark.txt`.
MarkChase200's disabled windows average about 47.964 ms/frame, versus 55.073 ms in
the preceding image. This is an approximate 12.9% reduction; the PC profiler's
two-second windows can contain partial 32-frame cycles, so different throughput
also changes the small partial-cycle weighting. Fixed-cycle confirmation remains
desirable. Other native/complex cases have not gained similarly from this change.

The focused laptop mark-chase slope is 218.04 us (-6.98% mean versus its saved
pre-change fixture baseline); pulse is 13.607 us. A new same-binary paired resource
fixture measures complete per-pixel initialization at 14.985 us and scalar-prefix
reuse at 6.7068 us for 200 pixels. The pair checks identical nonblack output across
all 32 frames and shares its fixture with the structural hoisting test. This is a
comparison of the complete uniform prefix, not an isolated resource-only speedup.
These two new fixture baselines were saved after the change under `effect-vm` for
future comparisons and are not mislabeled pre-change measurements.

An explicit new allocation test covers the resource fixture with curve automation,
both direct and recursive identity sampling, from the first prepared frame and
across backward seeks. All ten allocation tests, nine uniform tests, three native
parity tests and full repository checks pass. Both firmware binaries pass release
clippy; the four PC collector tests pass.

The normal fixed-32-frame, 160-case image built and flashed successfully. It is
preserved as `target/normal-resource-mark.elf`, SHA256
`0c650b101df74df316906fd17c2a48fed8b9c99cc987b52792109f99d4a69ae3`.
Its capture is running. The normal collector now accepts an ELF and optional
exclusive raw-output filename, recording the hash directly with the capture;
both normal and PC workflows and limitations are documented in the firmware README.

The normal capture completed successfully and is saved directly as
`results/2026-09-05-normal-resource-mark.txt`: all 160 cases passed host checksum
checks, timed allocations were zero, prepared first-frame allocations were zero,
and the ending heap recovered all 163840 bytes. Unlike PC-window last-frame checks,
this harness checks every timed frame and uses the fixed 32-frame cycle. It does
not yet include the separate native chase/pulse and mark fixtures from the PC image.

Next memory target found in current source: effect/operator automation workspaces
are sized for every effect/node, including empty `None` entries. This consumes
prepared heap and touches empty entries in normal sampling. Packing only actual
automation state during preparation may remove this cost; it needs measured memory
and timing evidence before changing the representation.

The complete fixed-cycle comparison with `2026-09-04-array-storage.txt` shows
substantial operator improvements: nested2/PixelRamp at 200 pixels fell from
6125 to 3631 us (-40.72%), and mixed-native/PixelRamp from 6119 to 3632 us (-40.64%).
Across 400/800/1600 pixels these cases improve by roughly 40.7%-40.8% as well.
These are complete image comparisons, including the documented IRAM placement
change, not an isolated attribution to one compiler edit. The largest measured
slowdown among the 160 cases is layered16/UniformFade at 1600 pixels: 14916 to
15358 us (+2.96%); the same family is +2.85%-2.93% at smaller sizes. This is a
consistent regression, not dismissed as noise. Its retained heap rises by 136
bytes, consistent with the per-node automation timestamp storage introduced here.
Whether that storage/scan causes the timing increase still needs an isolated test.

## Packed automation workspaces

Before this change, a controlled 200-pixel fixture with no automation retained
3876/5196/15052 bytes of desktop evaluation workspace for 1/16/128 effects,
respectively, despite unchanged pixel buffers and VM requirements. It made eleven
workspace allocations in each case. Empty effect-state entries accounted for the
88 bytes per additional effect.

Elaboration now assigns dense indices only to automated effects/operators. Workspace
construction stores only those states; no per-effect/node placeholder entries remain.
Effect evaluation borrows the relevant state directly, removing the temporary
take/restore closure and lazy default insertion. Recursive operators still move their
actual state out while sampling upstream, then restore it, because upstream traversal
borrows the rest of the workspace. No per-frame search, extra per-state allocation,
or new execution category was introduced. Effects' automation cache time is also
invalidated before a potentially failing update, rather than retaining an old stamp
after partially mutated parameters.

The same unautomated fixture now retains 3724 bytes and makes nine workspace
allocations at all three effect counts. Its regression test asserts that storage
does not grow and that the first evaluated frame allocates nothing. Existing
resource/native automation tests pass, and the sibling-operator seek test now uses
two distinct dense automation slots. Full repository checks are running; the normal
firmware built successfully and its flash is in progress. Timing is not yet claimed.

The first full check found one collapsible-if lint in the operator state handling;
it was simplified and the repeated full check passed. Both firmware binaries pass
release clippy. The normal image flashed successfully and is preserved at
`target/normal-packed-automation.elf`, SHA256
`686fec9caff4a58aed9dd14e67ec28b90ba15eeba250f806f5b86e7ee0e0e0aa`.
Its fixed-cycle capture and focused laptop timings are running.

The next output-path simplification is visible in current code: the sequence's
rendered graph buffer is copied into `ShowWorkspace.colors`, then copied again into
element state. Returning the already-rendered colors as a borrowed slice would
remove the intermediate color buffer and copy without a new output execution mode.

Packed automation validation completed: the first normal capture was rejected for
corrupt serial bytes near its end. The same preserved ELF passed a complete repeat,
`results/2026-09-05-normal-packed-automation-repeat.txt`: all 160 cases, no checksum
mismatches, no prepared first-frame or warmed allocations, final heap 163840 bytes.
Scan 200 median is 5252 us versus 5255 us in the immediately preceding image;
retained setup memory fell from 12140 to 12048 bytes. The 16-layer UniformFade
1600 case is 15430 us versus 15358 us immediately before packing and 14916 us in
the historical baseline. Packing does not resolve that regression; it remains open.
Focused desktop Criterion means are controller 8.8455 ms (CI 8.7816–8.9113 ms)
and 16-layer UniformFade 11.681 us (CI 11.634–11.738 us). These are not a paired
fresh pre/post experiment; the controller still reports +2.518% versus historical
`effect-vm`, while the fade reports -2.5014%.

### Borrow the graph output instead of copying through another buffer

`PreparedSequence::evaluate(time, workspace)` now returns a borrowed color slice
from its existing graph storage. `PreparedShow` copies those colors directly into
element state; its redundant color vector is gone. The owned preview-frame adapter
also clones directly from graph storage, without retaining an intermediate vector.
This removes one pixel-sized allocation and copy from show playback, while keeping
the physical-output API unchanged. There is no alternative executor or compatibility
wrapper. Out-of-duration evaluation clears and returns the same output slot.
Allocation tests cover the first evaluation, duration boundary, maximum timestamp,
and a backward seek. Full repository checks and hardware measurement are in progress.
The built normal ELF is preserved at `target/normal-borrowed-output.elf`, SHA256
`75816c43000cb2fa8c1d36f7d9b0e3d9852c7fbf796170ab0d9afcaa6ac00e07`.

Full `pnpm check` passed, as did both firmware release clippy targets and the
focused 12 allocation / 10 uniform-execution tests. The borrowed-output test
confirms zero allocations for first use, blank end-of-show output, and backward seek.
Current tracked-subtree count is 17141 nonblank Rust lines in 55 files, versus
16653/54 at this goal's start (+488). This includes profiler and shared fixture code,
but excludes tests, benches, Python, and docs; it is not a whole-repository LOC claim.

Two unchanged-executable focused Criterion runs measured controller means 9.1021
and 8.6426 ms, and 16-layer UniformFade 12.215 and 11.864 us. The second controller
run matches the historical baseline (+0.1663%, p=0.74). Thus the first run's apparent
increase is not consistently reproduced on identical code; the source of the
between-run timing shift is still unidentified. This is not evidence that every
older regression was noise, nor a basis for claiming a precise speedup from removing
the copy. Affinity and raised thread priority were already enabled in both runs.

The borrowed-output hardware capture `results/2026-09-05-normal-borrowed-output.txt`
passed all 160 cases, every timed-frame checksum, zero prepared-frame allocations,
and full heap recovery. Compared with the preceding packed-automation ELF, all
show paths save exactly 3 bytes per pixel (600/1200/2400/4800 bytes at the four sizes).
ScanSweep medians are exactly unchanged at 5252/10493/20973/41935 us; nested-two
PixelRamp changes by only +0.06–0.08%. However, simpler output-dominated paths are
slower: 16-layer UniformFade 2045/3957/7781/15430 -> 2084/4037/7941/15750 us
(+1.9–2.1%), and native automation 210/406/796/1578 -> 228/429/832/1637 us
(+8.6% at 200 pixels, falling to +3.7% at 1600). These are real observed image
differences, not dismissed as noise. Removing the intermediate copy is simpler and
saves memory, but its resulting code generation/placement and these regressions
still require attribution before deciding the next optimization. Both exact ELFs
and complete raw captures are preserved for controlled comparison.

### Full-cycle attribution windows

The normal ELF's fully inlined `PreparedShow::evaluate` grew from 0x475e to
0x4c12 bytes (18270 -> 19474 bytes) after borrowing the output. Both versions remain
in IRAM at 0x40088cf0/0x40088cf4 respectively, so this is not a move of the show
evaluator from IRAM to flash. Static disassembly counts are 6645 -> 7100 instructions,
386 -> 402 stack-store instructions, and 838 -> 809 stack-load instructions. These
counts describe the entire compiled function, not executed hot-path frequencies;
they establish a code-generation change but do not yet explain the slowdown.

The PC harness now completes whole 32-frame cycles before ending each two-second
window, reading the clock only between cycles. This removes unequal show-time
mixtures from cross-window comparisons, particularly overlapping marks. The existing
4096-sample bound is still checked; overflow is rejected, never truncated silently.
The collector rejects partial cycles and its five contract tests pass. Full repository
checks and both firmware clippy targets pass. The new image was flashed and preserved
at `target/pc-borrowed-full-cycles.elf`, SHA256
`6ef0ddaa29b21fd71c17ba8467a9eaecf0c78b4d1b00e9ab2b1de273dfe9fc20`.
Its capture `results/2026-09-05-pc-borrowed-full-cycles.txt` is running. Older
partial-cycle profiler means should not be treated as identical-workload baselines
for this changed harness. Normal fixed-cycle captures remain the runtime comparison.

Both the first full-cycle capture and `-repeat.txt` were rejected for non-ASCII
serial corruption (first after ChasePulse1 began, repeat during SparkleComet).
Neither is a complete accepted profiling result. Earlier valid records can locate
instructions but do not establish full fixture coverage. Mapping the first capture's
eight most frequent UniformFade/997-us PCs through the exact ELF's inline debug
information placed six in `PatchSource::write` (element/cell checks, iteration, push),
one in layer evaluation, and one in filter evaluation. This identifies repeated
per-cell patch gathering as a concrete next preparation target, not an allocation.

A controlled code-generation experiment adds `#[inline(never)]` to the existing
`sample_signal_graph` function and keeps that function/literals in IRAM with the
firmware linker script. It adds no new execution API. The resulting normal image is
preserved at `target/normal-outlined-graph.elf`, SHA256
`df40af80da6651f514d7cc8211e74922ee2e99f2f95074beb91da673ccd56c80`.
Show evaluation is 0x24d3 bytes and graph evaluation 0x2b18, both in IRAM; their
combined size is larger than the original fully inlined function. Full repository
checks pass. Desktop measurements and the normal-board experiment are in progress;
the annotation is experimental until these results are assessed.

The outlined-graph desktop run completed: controller 8.5321 ms (CI 8.4714–8.5975),
16-layer UniformFade 11.631 us (CI 11.572–11.698). This single run is consistent
with no material desktop penalty; it does not isolate a small gain from the already
observed between-run variation. The normal firmware flash is underway after the
second corrupt PC capture, so board timing—not these desktop numbers—will decide
whether the experiment addresses the embedded regression.

The first outlined-image flash failed with `FlashDeflData: Bad data checksum` at
115200 baud. A retry of the same preserved image is running at 57600 baud. Combined
with corrupt UART captures, this is evidence of unreliable board communication, not
proof of a runtime failure or a particular cable/driver cause. No corrupt capture
has been repaired or accepted. Firmware clippy also passed for this experiment.

The 57600-baud flash succeeded. The normal fixed-cycle collector is now running
against `target/normal-outlined-graph.elf`, writing
`results/2026-09-05-normal-outlined-graph.txt`.

That capture passed all 160 cases, every checksum, no prepared-frame allocations,
and full heap recovery. Keeping graph evaluation separate removes the observed
borrowed-output regression: UniformFade show 200/1600 is 214/1624 -> 195/1471 us;
16-layer UniformFade is 2084/15750 -> 1967/14804 us; native automation is 228/1637
-> 197/1472 us. ArrayLifetimes 1600 is 1722 -> 1569 us. No case increased beyond
integer timing rounding in this comparison. The graph boundary is retained with
its IRAM placement. This identifies the combined function's generated execution as
the practical regression source; it does not establish a specific hardware stall
counter or prove every changed instruction's individual contribution.

### Prepare patch source spans

Patch sources now contain ordered `(element, start..end)` spans instead of individual
cell addresses. Elaboration merges only immediately adjacent cells of the same
element, preserving reversals, repetitions, and element boundaries. Color/scalar/
indexed gathering checks and copies a whole span; fixture spans remain one cell.
No runtime search, source classification, second representation, or new dependency
was added. A contiguous 200-pixel source shrinks from 1600 address bytes to one
12-byte span. Fully fragmented selections can instead cost 12 rather than 8 bytes
per selected cell; this explicit representation tradeoff favors the normal contiguous
strand while preserving general ordering. Successful prepared-frame allocations
remain zero in focused tests.

The starter-project elaboration test now checks exact expanded source addresses and
maximal coalescing. The patch test covers reordered/repeated cross-element spans,
zero-allocation first gathering, and invalid span bounds, alongside existing color,
scalar, and fixture encoding coverage. Full checks are running. The normal span ELF
is preserved at `target/normal-source-spans.elf`, SHA256
`db5e56bf0d5a698c1225139a19292139859937edf2fb0aca833ccf20bf7ac64b`.

Full `pnpm check` passed for source spans. The firmware flash succeeded; fixed-cycle
capture `results/2026-09-05-normal-source-spans.txt` is running, along with the
focused desktop Criterion comparison. Early ScanSweep 200 results show the expected
1588-byte retained-memory reduction (11448 -> 9860 bytes); no complete-capture
performance claim is made yet.

The source-span capture completed successfully: all 160 cases, host checksum parity,
zero prepared-frame allocations, and final heap 163840. UniformFade show 200/1600
improves from the outlined-graph image's 195/1471 to 124/897 us; native automation
197/1472 -> 126/899 us; 16-layer UniformFade 1967/14804 -> 1896/14231 us.
ArrayLifetimes 1600 is 1569 -> 995 us. These use identical fixed show cycles.
Desktop Criterion means are controller 8.0414 ms (CI 7.9945–8.0907) and 16-layer
fade 11.078 us (CI 11.038–11.116), both below the historical `effect-vm` baseline.

### Reuse timing with the existing upstream sample identity

The existing effect/time cache now retains progress and local time as well as VM
prefix identity; the existing operator/time slot retains progress. Repeated upstream
pixels at that same effect/node and time no longer repeat floating-point progress
division. A different key, an error, or a new frame invalidates the same cache as
before; storage stays one entry per existing VM slot, not one entry per effect or
sampled timestamp. Native operators do not calculate an unused progress value.
This adds fixed scalar storage rather than a new map, cache abstraction, or allocation.
Focused effect seek/identity/allocation tests pass; full checks and board validation
are in progress. The next normal ELF is preserved at `target/normal-reused-timing.elf`.

The first timing-cache build used nested tuples. Its full check passed tests but
failed clippy's type-complexity lint. Identity/time/progress now use the small named
`CachedVmSample` data record shared by the existing effect/operator slots; effect
local time stays alongside it. No extra allocation or methods were introduced.
The tuple image (`normal-reused-timing.elf`, SHA256
`27491d1e5c66fd332b558ac99aa3847fc7b6b00c16125856e2d6355f6e4fb996`)
flashed successfully but has not been measured. The named-record version is being
rebuilt and rechecked before current-source hardware measurements.

The named-record timing-cache version passes full `pnpm check` and both firmware
release clippy targets. Its image, `target/normal-cached-timing.elf`, SHA256
`f20e9dc1a0833a61da1ac8754f9951b15d11eb765d28c68617620e7a1f3898d3`,
flashed successfully. Fixed-cycle capture `results/2026-09-05-normal-cached-timing.txt`
and focused desktop controller/operator/fade benchmarks are running.

The PC collector now reports source-level leaf attribution in addition to linker
symbols. It batches unique PCs through the exact ELF's `addr2line` debug information,
checks returned addresses/coverage, and weights locations by all observed samples.
This exposes inline loops otherwise attributed to a giant caller. Six collector
contract tests pass; a real-tool smoke check distinguished a slice lookup from layer
evaluation in three PCs previously attributed entirely to `PreparedShow::evaluate`.
It remains sampled leaf attribution, not reconstructed dynamic stacks. The two failed
full-cycle captures remain rejected; no new complete current PC capture is claimed.

The in-progress timing-cache desktop run has completed controller (7.8791 ms,
CI 7.8052–7.9601) and 16-layer fade (11.092 us, CI 11.011–11.179); operator cases
are still running. These compare to the historical baseline, not a fresh isolated
timing-cache baseline. The normal board collector is also still live. The new
source-attribution collector has six passing tests and a real-tool smoke check;
the next full repository check should follow completion of host timing to avoid
competing with benchmarks. No runtime source edits occurred after its last full check.

The timing-cache desktop benchmark completed. Operator full/reuse at 1600 pixels
measured 144.08/127.87 us; historical-baseline comparisons report -11.03%/-16.56%.
The first timing-cache normal capture was rejected for a corrupt line at 1600-pixel
operator sampling; `-repeat.txt` was rejected during boot before measurements.
`results/2026-09-05-normal-cached-timing-repeat2.txt` is now running on the same ELF.
This is an unreliable transport observation, not an accepted partial performance
result. Full repository checks are running after completion of host benchmarks,
and the current full-cycle PC image is being rebuilt for source-level attribution.

The timing-cache `-repeat2.txt` capture completed successfully: all 160 cases,
every checksum, zero prepared-frame allocations, final heap 163840. Relative to
source spans alone, nested-two PixelRamp improves 3255/25933 -> 2919/23228 us
at 200/1600 pixels (-10.3–10.4%); nested-eight improves 12439/99271 -> 11012/87811
us (-11.5%). Retained heap increases 4 bytes per operator VM slot, as expected.
Alternating/grouped temporal sampling, which continually changes the cache key,
is +0.09–0.10%; this is the observed cost of the extra timing cache handling without
its same-time reuse benefit, retained in exchange for the larger nested gains.
ArrayLifetimes show 400 and 1600 are 332/995 -> 345/1007 us, while 200/800 are
unchanged or faster. That non-operator case does not execute the new timing-reuse
branch; its small image difference is unresolved and is not dismissed as noise.

Full repository checks passed again after the source-attribution collector change.
Current subtree LOC is 17237 nonblank Rust lines / 55 files versus 16653/54 at goal
start (+584). Clarification of earlier LOC wording: the selected `src` subtrees
include module/unit-test files such as `output/patch_tests.rs`; they exclude integration
test directories, benchmark directories, Python collectors, and documentation.

The current full-cycle PC image is preserved at `target/pc-cached-timing.elf`, SHA256
`a3901c1a2407335e9841fdbeba4beb7b7ee56a2889551b3961e79568a68ec5b5`.
Its 57600-baud flash is running after completion of the normal capture.

The PC flash succeeded; source-level/full-cycle capture
`results/2026-09-05-pc-cached-timing.txt` is now live on that preserved image.

The current full-cycle/source-attribution capture completed successfully: all 56
windows across 14 fixtures, no sample overflow, allocation assertions and final-frame
host checksums passed. This is the first accepted full-cycle capture after the earlier
partial-cycle capture failures. The raw file and exact ELF are preserved above.

### Reuse pixel-independent upstream effect results

The existing effect/time cache now retains its last RGB result too. For DSL effects
whose compiler metadata says they do not depend on pixel context, recursive upstream
sampling reuses that color for the same effect/time rather than re-entering the VM.
No result reuse is inferred for operators (their signal inputs implicitly depend on
the current pixel) or newly classified native effects. Different keys and new frames
still invalidate reuse; storage remains one fixed entry, not a per-effect result map.
The direct/scalar parity test now covers identity-operator wrapping at 1/4/16 layers
and backward seeks. Focused correctness/allocation tests pass; full checks are running.
A paired Criterion fixture conservatively marks the same bytecode pixel-dependent
to force recomputation and verifies 32-frame checksums before timing both paths.

Full repository checks passed for uniform-result reuse. The normal hardware harness
now adds paired `uniform_full`/`uniform_reuse` identity-operator cases at all four
pixel counts, using the existing UniformFade host goldens. The former changes only
the conservative pixel-dependence metadata to force repeated VM calls; the latter
uses the compiler's real metadata. Collector coverage increases from 160 to 168
cases, without dropping any prior case. Firmware checks and the new paired Criterion
benchmarks are running. Their first `effect-vm` baseline is post-change and intended
for future comparisons; the within-run full/reuse pair is the current causal control.

The paired desktop benchmark completed. Arithmetic means read from Criterion's
`new/estimates.json` are full/reuse 12.064/7.569 us at 200 pixels and 95.107/58.886 us
at 1600: about 37–38% less time for this pixel-independent upstream fixture.
This is not a claim about all effects. Firmware release clippy passed; the image
`target/normal-uniform-results.elf`, SHA256
`2e16ad28cb51e65ae05bdf8256665067b11b554c79fe3b76792e570590119509`,
flashed successfully. The 168-case capture `results/2026-09-05-normal-uniform-results.txt`
is now running. Earlier references to Criterion console 'means' should be read as
its reported time estimates (typically regression slopes); this paragraph explicitly
uses JSON arithmetic means. The measurement statistic will be made consistent in
the final comparison rather than mixing those estimators for precise percentages.

The first 168-case uniform-result capture was rejected for serial corruption during
the 800-pixel PixelRamp cases. The same ELF is being repeated in
`results/2026-09-05-normal-uniform-results-repeat.txt`; partial uniform-pair results
are not yet treated as a completed hardware validation.

MarkPulse's section index now uses 32-bit Euclidean integer division, as the chase
section index already does, rather than converting integer pixel/width values to
float and flooring a software float division. Geometry work is also confined to
the nonzero edge-fade branch. It adds no prepared storage or new runtime structure.
The DSL-reference schedule/sample parity test is expanded across widths 1/3/5/17
and edge fades 0/1/3, with the original times, targets, and exact color assertions.
The focused original case passed; the expanded full check and new PC firmware build
are running. No performance gain for this latest arithmetic change is claimed yet.

The expanded full repository check passed, as did firmware release clippy. The new
PC image is preserved at `target/pc-integer-mark-pulse.elf`, SHA256
`bf2b39716acce9e353075c85ed8781b1084b36131f1e5612490216212d362a1c`.
It has not been flashed yet because the normal uniform-result repeat is still live.

The uniform-result `-repeat.txt` capture passed all 168 cases, every checksum,
zero prepared-frame allocations, and full heap recovery. Paired full/reuse medians
are 1170/862, 2325/1708, 4634/3399, and 9252/6781 us at 200/400/800/1600 pixels
(26–27% improvement). Comparing existing cases to timing-cache-only shows a cost:
pixel-dependent operator reuse is +1.6% across sizes; full operators +1.1–1.2%;
nested-two +1.0%. The new warm path reread program pixel-dependence metadata on
each hit. Its cache now stores an optional uniform color: metadata is inspected only
on the first sample for an effect/time key; later hits either return that color or
continue pixel-dependent execution without a program lookup. The benefit is retained
while this follow-up seeks to remove its measured overhead. Checks and firmware build
are running; the change is not assumed faster until measured.

The preserved integer-MarkPulse PC image flashed successfully and is capturing to
`results/2026-09-05-pc-integer-mark-pulse.txt`. That image predates the optional-color
cache follow-up, so comparisons must use its exact source/image boundary.

The integer-MarkPulse PC capture passed all 56 windows. Disabled controls measured
MarkPulse 2239.08/2239.06 us versus 2231.95/2231.92 us in the prior full-cycle image;
MarkChase 48600.78/48600.52 versus 48562.88/48562.56 us. No gain is claimed.
Inspection of the shared fixture found its edge fade is zero, so it does not exercise
the new integer section-index branch. This is a coverage gap for measuring that
arithmetic change, despite the expanded nonzero-fade correctness tests. A nonzero
edge-fade profiling case is needed before attributing its performance. The small
disabled-branch image differences remain observed rather than declared noise.

The optional-uniform-color cache refinement passes full repository checks and both
firmware clippy targets. Its normal image is preserved at
`target/normal-optional-uniform.elf`, SHA256
`57155b2a7625046db7f1403490c5e2e5f8dae1dce728b6d63e44d6e8150b292c`.
It flashed successfully; `results/2026-09-05-normal-optional-uniform.txt` and focused
desktop comparisons are now running. This image also includes integer MarkPulse,
which the normal non-mark fixture suite does not exercise.

The optional-color normal capture passed all 168 cases, checksums, allocation and
heap checks. It did not improve the intended pixel-dependent path: operator reuse
200/1600 is 1595/12664 -> 1634/12977 us compared with unconditional cached colors,
while uniform reuse is essentially unchanged at 863/6788 us. This failed hypothesis
is retained in the record. Moving result reuse into the VM's sample entry point,
which already has dependency metadata and prefix-reuse permission, is the next
simplification to investigate instead of layering more graph-cache branches.

To close MarkPulse measurement coverage, shared mark fixtures now include
`MarkPulseEdge200` with edge fade 1.0. PC coverage is 15 fixtures / 60 windows;
normal coverage remains 168. Golden generation, desktop fixture and collector checks
include the added case; all six collector tests and full repository checks passed.
Two controlled PC images were built with identical fixtures: integer section division
at `target/pc-edge-integer.elf`, SHA256
`20064242af237a6d10a23c63fa89e8df027e76661b80657baf9b932256f4c4eb`,
and the prior float/floor formula at `target/pc-edge-float-control.elf`, SHA256
`9c9102c89891cdeba2d7cfe3580648a39cd874dc5c654e721a9506c704f17944`.
Only that arithmetic expression was changed for the control build, then integer
source was immediately restored and its expanded reference test passed again.
No temporary float-control edit remains in the worktree. The integer image flash
is underway; neither new 60-window comparison is claimed complete yet.

The integer image capture now passed all 60 windows in
`results/2026-09-05-pc-edge-integer-retry.txt`. The first capture invocation omitted
the exported tool environment and failed before opening serial; its empty raw file
is not a measurement. Nonzero-edge disabled controls are 2028371/800 and
2028367/800 us per frame (2535.46 us). The float-control flash subsequently timed
out in FlashDeflData; an identical-image retry is underway, not yet measured.

Uniform result reuse now lives in the bytecode sampling entry points, using the
existing compiler dependency metadata and caller's same-program/time reuse flag.
The graph cache again contains only sample timing. This also covers uniform DSL
operators without adding runtime effect classifications. A focused test verifies
reuse, fresh-invocation execution, and invalidation on error. Initial full checks
passed tests but found two collapsible-if lints; these were corrected and full
checks restarted. No performance improvement is claimed yet. The normal firmware
build is preserved as `target/normal-vm-uniform.elf`, SHA256
`6b42d684398e04a2967e8707c072126a70b18b5cb1d3d107c0b8ab6d6e55855d`;
it predates the syntax-only lint fix and added unit test, but contains the new
runtime behavior. It has not yet been flashed or measured.

Full repository checks and standalone firmware clippy now pass. The identical
float-control image flashed on retry and its 60-window capture is running. Initial
desktop VM-level cache measurements regressed versus the initial uniform-cache
baseline: Criterion reports approximately +13/+17% full evaluation and +32% reuse
at 200/1600 pixels. Moving the cache hit later reintroduces effect parameter/context
setup before the return, rather than the former graph-level early return. This is
a concrete extra-work path, though exact timing attribution awaits controlled
repeats and on-device results. A second unchanged-source desktop run is underway.
The first filter included a stale render benchmark name and therefore ran only the
four uniform cases; the second uses `controller_output_dense_60_frames` as well.

The second desktop run also shows the uniform-path regression (+24.5/+28.2% reuse,
+14.3/+13.6% full in Criterion's mean-change comparison at 200/1600). Absolute
estimates moved between runs, but both show the same direction; the regression is
not dismissed as noise. Controller output's console slope is 7.8327 ms; use saved
JSON arithmetic means for final cross-run reporting. Firmware clippy passed.
The float-control capture and first repeat both failed strict ASCII decoding on
0xff serial corruption and are rejected entirely. Repeat2 is underway using the
same preserved ELF. On-device VM-cache measurement remains pending after this
control capture; no claim of completed cache optimization is made.

The third float-control capture also failed on 0xff and is rejected. The integer
edge result remains valid, but its matching float comparison remains unverified.
To avoid serial retries monopolizing progress, the board was switched to the
preserved VM-cache normal image. `results/2026-09-05-normal-vm-uniform.txt` passed
all 168 cases, exact timed-frame checksums, zero prepared-frame allocations, and
full heap recovery. Uniform reuse 200/1600 is 988/7787 us versus initial graph-cache
862/6781 us; full evaluation is 1245/9859 versus 1170/9252. Pixel-dependent
operator reuse 200 is 1672 versus 1595. This corroborates the desktop regression.
VM cache adds four retained bytes to the one-operator fixture (7240 -> 7244).

Based on the extra setup path and duplicate validity storage, complete effect
result reuse is consolidated back into the graph's existing successful sample/time
cache. It stores a plain Color with its timing key and checks the existing compiler
dependency metadata before returning early. The failed optional graph-color and
VM-color variants are removed; there is no new per-VM cache. The temporary unit
test for that deleted VM field is removed with its implementation; existing
whole-show seek tests and full/reuse exact-output fixture coverage remain. This is
an evidence-backed choice of cache ownership, not dismissal of a regression.
Full checks and a fresh normal image build are underway. Before this consolidation,
the same four source subtrees counted 17346 nonblank lines / 55 Rust files; refresh
that number after removal and final validation rather than reporting it as final.

Consolidated-cache repository checks and both firmware clippy targets pass. Source
count is now 17272 nonblank Rust lines / 55 files, 74 fewer than the failed VM-cache
experiment but still 619 above the goal baseline, including profiling and tests.
Normal image `target/normal-consolidated-cache.elf` SHA256
`84e322647c67ae63fa97051868014d4a9d6eb99710b690bc73a891322c2c8706`
is preserved. Its first flash failed FlashDeflData checksum; same-image retry is
running. Full desktop comparison is starting after establishing the newly added
`prepared_device_marks/pulse_edge` baseline at current source. That one new tag is
post-change, not pre-optimization evidence. All existing effect-vm tags are retained.

The consolidated image flashed successfully on retry; its strict 168-case normal
capture is running. Full desktop comparisons are running against retained tags.

Profiler overhead was recalculated from the accepted integer-edge image's 60
whole-cycle windows, using the mean of its two disabled controls for each fixture:
997-us sampling adds 0.359-0.395%; 1999-us sampling adds 0.178-0.201%.
Disabled control differences within that image are at most 0.003% in magnitude.
This quantifies measurement perturbation, not cross-image code-placement effects
or absence of periodic sampling bias. Disabled 200-pixel fixture means are
ChasePulse1 1239.18 us, ChasePulse4 2752.02 us, ChasePulse16 10934.95 us,
MarkPulse 2241.85 us, MarkChase 48640.36 us, and MarkPulseEdge 2535.46 us.
These profile-image results are not substituted for normal-image timings.

The consolidated normal capture completed successfully: all 168 cases, timed
goldens, prepared allocations, and final heap recovery passed. The new concise
`runtime_optimization_results_2026-09-05.md` records exact baseline/current board
means, memory, profiler overhead, limitations and source counts. No normal-case
regression exceeds a one-microsecond raw-VM control difference versus goal start.
The full desktop run is still live and has flagged pixel-layer regressions (e.g.
PixelRamp/4 approximately +7.4%); these require focused repeats and explanation,
not dismissal based on the board results. The float control flashed successfully
and `pc-edge-float-control-repeat3.txt` is now capturing the same preserved image.

Float-control repeat3 passed all 60 windows. Integer/float nonzero-edge disabled
means are 2535.461/2845.509 us, a 10.90% reduction. Zero-fade MarkPulse is
2241.849/2234.720 us (+0.32% integer-image difference); MarkChase is
48640.359/48602.266 us (+0.08%); ScanSweep differs by about 0.002%. This closes
the missing arithmetic comparison without attributing all image-layout effects to
the changed expression. The accepted raw capture, failed predecessors, and exact
ELFs remain separate. Board currently contains the float-control profiler image;
restore the final normal profiling firmware before handoff.

Full desktop comparison passed. Consistent JSON arithmetic means are recorded in
the concise report. The unchanged host executable repeat confirms multi-layer
pixel regressions: PixelRamp/4 +7.20%, /16 +8.35%; ArrayRamp/16 +8.86%. A one-line
`#[inline(never)]` experiment on the existing `sample_layer_frame` is now being
benchmarked to test layer-loop code generation inside the large graph evaluator.
It is not yet retained or validated on ESP32. The previously verified board image
was restored successfully (normal-consolidated-cache ELF); no PC capture remains
live. Host consolidated baseline executable is preserved for direct A/B repeats.

The outlined-layer experiment passed its desktop comparisons: PixelRamp 1/4/16
is now about 9.8/7.7/7.1% faster than goal baseline rather than slower; ArrayRamp
improves 11.0/10.3/7.7%. Controller output is about 8.9% faster than goal baseline,
but somewhat slower than the preceding consolidated image's 10.8% gain. This
tradeoff remains visible. The existing layer function is being retained for board
validation with its text/literals explicitly placed in IRAM. Full checks and the
normal firmware build/clippy are running. This is a single shared code-generation
boundary, not a second runtime or added dispatch abstraction.

The final audit skill review traced preparation/runtime ownership, dense automation
slot assignment, cache identity/reset and seek/error tests, span routing, and
profiler assertions. The concise report now explicitly states the trusted-prepared
artifact boundary, bounded last-sample reuse, and PC harness last-frame-only golden
check. These are real coverage/contract limits, not claims of universal validation
or caching. No serious new defect was found in the reviewed changed paths.

Outlined-layer full repository checks and both firmware clippy targets pass.
Preserved normal ELF `target/normal-outlined-layer.elf` has SHA256
`a0022a0ceb660f02b51bda936e3c2b096540289180b037976511ffd17dcf59e7`.
The linker map confirms graph at 0x4008b290 (0x2590 bytes), layer evaluator at
0x4008d820 (0x6b5 bytes), both IRAM. Flashing and a complete new desktop comparison
are underway. The preceding consolidated image and full report numbers remain
historical until this new image's capture and complete desktop run finish.

Outlined-layer normal capture passed all 168 cases, timed checksums, zero prepared
allocations and heap recovery. Complex cases are unchanged versus consolidated
cache, but uniform show 200/1600 rises 123/896 -> 132/957 us and sixteen uniform
layers 1895/14230 -> 1992/14981 us. The latter is 0.44% above goal-start timing at
1600 pixels, so an unconditional improvement is not claimed. Desktop's full run
is still running from that image's host source.

Inspection finds the uniform effect loop still selects an Option<Color> and tests
cache state for every pixel after its first sample. The next simplification uses
the existing dependency flag before the loop: sample a uniform effect once using
the first target pixel, then compose that color across its target. Pixel-dependent
effects retain only the repeated-geometry cache, with no uniform-color branch.
Empty targets still execute no sample; the first sample and errors keep the same
ordering. No new runtime effect category, workspace or cache is added. This source
change is not yet built or measured; the running desktop executable remains the
outlined-layer version and must not be mislabeled as the new uniform-loop version.

The outlined-layer full desktop comparison completed successfully and its host
executable is preserved at `target/outlined-layer-render-bench.exe`. Compilation
of the new uniform-loop version then began without overlapping benchmark timing.
All 11 `prepared_uniform` tests pass, including a new empty-target/nonempty-target
error test. Full repository checks and standalone firmware build/clippy are now
running. The README's flash examples now use the observed 57600-baud upload setting
and explicitly state that pyserial did not eliminate serial corruption; capture
still uses 115200. Firmware lint instructions cover both binaries and pc-profile.

Uniform-loop full repository checks and standalone clippy pass. Normal firmware
is preserved at `target/normal-uniform-loop.elf`, SHA256
`e0654c710d0f3f4b6507d5a05b0ccfcba70de214efc02bdca38b7ccf239ce07a`.
It flashed successfully without retry. `results/2026-09-05-normal-uniform-loop.txt`
and full desktop comparisons are running. No later runtime edit is planned while
these measurements establish this version's end-to-end outcome.

Uniform-loop normal capture passed all 168 cases, timed golden checks, zero
prepared-frame allocations and final heap recovery. Uniform show 200/1600 is
102/723 us; sixteen uniform layers 1525/11247 us. The direct uniform loop removes
the outlined version's loss and improves further, with no new cache or storage.
Complex shows improve another 17 us at 200 pixels after removing their uniform
cache branch. The largest baseline increase is one microsecond in a raw VM
control (0.03%). Current source count is 17292 nonblank Rust lines / 55 files,
639 above goal start. The concise report's board table and image identity now
refer to this final capture. Full desktop comparison remains live.

The final full desktop comparison passed. Its 56 current benchmark records all
have lower mean estimates than their saved effect-vm tags; retired microbenchmark
directories in target/criterion were excluded by checking this run's records,
not misreported as fresh regressions. Final host executable SHA256 is
`8f875b5238cec0c7f61e01bee8e267ce112ea76b6d05bc332de6cd756f6dfab2`.
The concise report now uses current JSON arithmetic means throughout. PC collector
tests (six) and firmware formatting pass. Final PC ELF SHA256
`43b04ae797d3c335ed62400121733861f09007320c86f710f233e5dccff3f23d`
is preserved at `target/pc-final.elf`; flashing it for a final attribution refresh
is underway. Runtime source is unchanged; restore normal-uniform-loop firmware
after the final profile capture.

Final PC capture `results/2026-09-05-pc-final.txt` passed all 60 windows. Current
sampling overhead is 0.355-0.408% at 997 us, 0.176-0.211% at 1999 us; paired
disabled controls differ at most 0.002%. Chase/pulse 1/4/16-layer disabled means
are 1.218/2.652/10.534 ms per 200-pixel frame, MarkPulse 2.206 ms, MarkChase
48.144 ms, MarkPulseEdge 2.499 ms. ScanSweep has 61-64% of samples in VM run and
11-14% in software float division; chase/pulse and MarkChase each have a sampling
window with about 41% in division. No heap allocation occurs in these windows.
The final report now uses this current-source profile, keeping the earlier isolated
integer/float A/B distinct. Restoring the validated normal-uniform-loop image is
underway; no runtime source change remains pending.

Final completion check: normal-uniform-loop firmware restored successfully to
COM4. Rechecked its ELF hash, final PC ELF hash, final desktop executable hash,
168 normal cases with full heap recovery, and 60 PC windows with terminal END.
No `f64` or `std::` use is present in runtime source. Final source/check boundaries,
semantic tests, memory limits, benchmark baselines, regression experiments and
profiler limitations are recorded in the concise results report. All required
validation processes have completed; no runtime edits or device actions remain.

## Prepared-sequence temporal frame reuse

The later real-show I2S investigation found a separate graph-level multiplier.
The starter TimeWarp operator asks its upstream signal for one time that is
identical for every output pixel. Recursive operator sampling nevertheless used
the scalar path, so four fixtures with identical 113-pixel local geometry sampled
the same native Spin work 452 times. At 25 revolutions this meant 11,300 Spin-loop
iterations per frame. The 66-second frame measured 39.840 ms on average over 20
HTTP requests (38.126-44.827 ms), with zero allocations.

Signal-sample bytecode now records a frame-cache slot only when the compiler's
existing uniform analysis proves that the time expression is pixel-independent.
At evaluation, that read fills a preallocated full-frame buffer through the normal
graph frame path and reuses it for later pixels. Pixel-dependent time expressions
retain scalar semantics. Separate slots preserve independent reads, cache keys
include upstream node and sample time, and keys are cleared at each top-level frame
because controls may have changed. Workspace admission includes these buffers and
the archive version is 2. No memory is allocated while evaluating a frame.

Spin also computes reciprocal scales once per pixel, tests the active interval
before sampling pulse shape, and avoids division for the common normalized
two-point curve span. These are algebraic changes to the existing general native
effect, not a new runtime effect class.

On the laptop, `render_representative_frames` fell from 2.924 ms before the change
to 1.001 ms (0.990-1.011 ms), about 65.8% faster. On the ESP32, the same 66-second
frame now averages 6.210 ms over 20 requests (5.624-8.830 ms), about 84.4% faster.
Ordinary playback remains 1.87-1.90 ms evaluation with about 2.47 ms encoding and
6.35 ms total. The most expensive continuous Spin window now averages 6.476 ms
evaluation and can still exceed the 8.333-ms total deadline after encoding; later
high-revolution windows average roughly 3.8-4.8 ms and meet it. The loaded heap is
81,620 bytes free versus 83,008 before the added 452-pixel frame buffer. The capture
`firmware/dawn-profile/results/2026-09-05-temporal-frame-cache-final.txt` passed
three Wi-Fi replacements, every rejection test, 200 checksum-checked frame requests,
and zero evaluation-allocation checks. The exact firmware ELF SHA256 is
`32a4312b56bb6644f714ced974a2e69bd7545aa06a7468518467874ad9827e2a`.

Native Chase/Spin geometry and timing were then hoisted inside the same shared
runtime. `NativeSampleCache` lives on the evaluator stack for one active effect,
rebuilds only when fixture pixel count changes, and retains no heap memory. It
precomputes section count, revolution scale, pulse duration, reciprocal duration,
and extension bounds while preserving the original per-pixel position division
and exact output checksums. Public scalar sampling uses the same implementation
with a one-sample cache; there is no ESP32-specific evaluator.

The laptop representative benchmark improved from 1.001 ms to 0.951 ms
(0.945-0.956 ms), about 5%. ESP32 direct evaluation at 66 seconds improved from
6.210 to 5.674 ms average and from 6.302 to 5.343 ms median over 20 HTTP samples.
The trimmed mean excluding the two largest radio-interrupted samples improved from
6.018 to 5.452 ms. Continuous hot windows improved 6.476/5.526 -> 6.014/5.142 ms,
and the later high-revolution window improved 4.819 -> 4.470 ms. Normal evaluation
is 1.84-1.86 ms, heap remains 81,620 bytes free, and 200 checked frames reported
zero evaluation allocations. The first hot region still totals 8.538 ms after the
2.464-ms encoder and therefore remains slightly above the 8.333-ms deadline. The
accepted capture is `firmware/dawn-profile/results/2026-09-05-hoisted-native-geometry.txt`;
firmware ELF SHA256 is
`d791c8042d2ed0ceae4eecbd94782e0cb29c2c3e791c6bd00601007b13b7488d`.
