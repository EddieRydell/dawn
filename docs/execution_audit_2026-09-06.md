# Execution audit follow-up

This change keeps native effects and operators. It does not introduce an embedded
interpreter, effect categories, or a second playback implementation.

## Execution model

`dawn-language` compiles DSL bytecode. `dawn-elaboration` expands generators and
prepares effects, targets, automation, signal connections, and output patches.
`dawn-runtime::sequence::PreparedSequence::evaluate(time, buffers, workspace)`
evaluates that frozen data on both desktop and ESP32.

The useful entry points are:

- `crates/dawn-runtime/src/sequence.rs`: signals, controls, fixture rules, patch.
- `crates/dawn-runtime/src/evaluation.rs`: frame traversal and the shared
  `EffectSampler`; native temporal operators sample whole upstream frames.
- `crates/dawn-runtime/src/dsl/vm.rs`: bytecode execution and reserved VM storage.
- `crates/dawn-runtime/src/sampling.rs`: canonical color and forward resource math.
- `crates/dawn-elaboration/src/native_effect.rs`: native generator expansion.

The sparse editor raster now uses the same effect sampler and automation state
construction as playback. It still prepares a sparse selection and scatters
samples to editor rows; those are display concerns, not another effect evaluator.

## Repairs

- Delay/Echo reuse whole-frame layer evaluation, including uniform work, native
  geometry caching and repeated fixture-coordinate reuse. Nested native operators
  use preallocated temporary frames, sized from recursive frame-sampling entry
  points rather than unrelated downstream composition nodes. Scalar sampling
  remains necessary for DSL queries whose time actually depends on the pixel. Exact nested parity tests
  compare these paths across backward and forward seeks.
- Native and DSL color multiplication now both round to nearest. The former
  native truncation could differ by one channel value. Exhaustive channel-pair
  testing covers this intentional correction.
- Curves and gradients use one forward sampler across direct resources, bound
  parameters, array values, native effects and controls. Equal-position gradient
  stops now consistently choose the last stop at the exact position. Redundant
  forward segment/gradient wrappers were removed; prepared inverse-curve crossing
  data remains because it accelerates an actual search rather than duplicating
  the forward representation.
- Automated native samples are local values borrowed by the effect sampler for a
  scoped callback. The pixel loop sees a plain borrow, not an owned/borrowed branch.
  Their resource references cannot survive into the next mutable automation update.
  The workspace no longer caches a second native sample that must manually be
  cleared before mutation. Missing prepared automation storage reports an error;
  evaluation no longer lazily clones a replacement parameter set.
- Spin appends its additional parameter after the Chase parameter layout, removing
  shifted-slot arithmetic. Compact positional bindings are retained, with explicit
  schema-order regression tests rather than runtime name lookup or another schema
  framework. Native operator input arity has one runtime definition and is checked
  against authoring metadata.
- Native generator expansion moved out of runtime. Even the PC profiler now loads
  host-expanded mark fixtures through the real sequence codec. Its build-only
  dependency on the existing elaboration crate does not enter device firmware.
  A single build-time exporter writes the mark and stacked chase/pulse archives
  and CRC sidecars, allowing the existing HTTP uploader to verify those same
  workloads. The existing crc32fast dependency is also used on the build host.
- MarkPulse without edge fading now uses the existing pixel-uniform broadcast
  path. Both mark samplers cache the pixel-independent parent-time hue calculation
  once per effect/time, matching the DSL's uniform-expression hoisting. Edge fading
  and pixel-dependent chase geometry remain per-pixel operations.
- Fixture behavior evaluation reads the current fixture's already-populated
  function state instead of maintaining and searching a second global list.
- Pure native plans need no operator VM slot. Frame-number APIs no longer silently
  clamp to the last frame when time-based evaluation would be outside the show.
- Loader validation checks target sample-cache bounds, effect durations, missing
  native automation parameters and native-versus-DSL VM slots. Workspace admission
  accounts for temporary frames and uses actual register, value, cache, element,
  fixture and patch layouts rather than guessed 32/64/256-byte multipliers.
- Calculated arrays already occupy fixed-width slots. Their former range allocator
  always received `allocate(1)`. A reserved `Vec<u32>` free-slot stack replaces the
  allocator's bins, range splitting/coalescing and allocation records. The runtime
  no longer depends on offset-allocator. Its unused vendored source was preserved;
  the obsolete dependency-only test was removed. Existing VM lifetime and
  allocation tests exercise the actual replacement.
- The HTTP frame-verification handler releases the playback mutex after evaluation
  and checksum calculation, before sending its response. Previously network write
  latency held that mutex and stalled the output task. Verification still competes
  for the same workspace while evaluating; it is not a parallel playback instance.

## Deliberate boundaries

Small linear effect/control scans remain. Whole-frame temporal evaluation removes
the per-pixel repetition of effect and target lookup in the confirmed bad path;
there is not yet evidence that a new interval tree or serialized active-set table
would repay its extra storage and complexity. The two graph traversals serve
different query shapes; their arithmetic and effect sampling are shared.

The original audit overstated two findings: authored curves already reject
duplicate positions, and enum identifiers already share their text rather than
allocating strings on assignment. No new enum interning system was added.

Archives are version 3 and must be regenerated with matching firmware. Admission
limits are for trusted compiler output, not a complete hostile-bytecode sandbox or
an allocator-level OOM guarantee. Workspace construction and loading still allocate;
successful prepared evaluation does not. Error reporting may allocate strings.

## Measurements and tradeoffs

The saved desktop baseline is Criterion `audit-start`. It was a quick smoke
baseline; later checks use 20 samples with one second warmup and two seconds of
measurement. A run overlapping the full build/check was discarded. Even quiet
repeated runs show host frequency/load variation, so small differences are not
presented as universal gains.

Consolidating forward resource sampling initially caused a measurable slowdown:
the first implementation searched even for endpoint samples. Checking endpoints
first and searching only the interior fixed that avoidable work. A separate
controlled executable comparison retained the pre-inline executable at
`target/audit-before-sample-inline.exe`: inlining the small shared sample method
improved the affected focused cases about 4-5%, without splitting the evaluator.

Final laptop measurements after uniform broadcast and hue hoisting:

| Workload | Initial | Final | Interpretation |
| --- | ---: | ---: | --- |
| Seven selected starter frames | 917 us | 985-1,064 us | 7-16% slower |
| Controller output, 60 dense frames | 7.127 ms | 7.966-7.983 ms | 12% slower |
| 16 uniform layers | 8.79 us | 10.50 us | 1.71 us more per frame |
| 200-pixel mark pulse | 12.41 us | 4.29-4.36 us | 65% less time |
| 200-pixel mark chase | 226.4 us | 185.0-188.5 us | 17-18% less time |
| 200-pixel mark pulse with edge fade | 13.20 us | 11.98-12.09 us | 8-9% less time |
| Full-layout generated mark pulse | 502.6 us | 282.8 us | 44% less time |
| Full-layout generated mark chase | 4.596 ms | 5.14-5.17 ms | 12% slower |
| 16 chase/pulse layers, 200 pixels | 40.14 us | 43.83 us | 9% slower |

Percentages use the displayed point estimates, not Criterion's bootstrap change
estimate. The seven-frame benchmark includes owned desktop snapshot copies; the
controller benchmark uses reusable output buffers and includes the patch stage.
Neither includes preparation. The different workloads are not interchangeable.

The mark-pulse gain comes from sampling identical colors once instead of for each
pixel. Hoisting parent-time hue also removes repeated curve sampling and HSV work
from mark effects. Other paths do not benefit from these shortcuts. Sharing one
sampler changes setup/dispatch and code generation; removing forward tables trades
their precomputed arithmetic for less retained data. Controlled inlining and
borrowed-versus-owned sampler experiments identify part of that cost, not a
complete cycle attribution of every difference. The remaining desktop slowdowns
are recorded as tradeoffs, not dismissed as noise or hidden behind the mark gains.

The scoped physical Rust line count (runtime, language, elaboration, profiling
firmware; including tests and benches, excluding generated target files) is
30,913 before and 31,028 after: **+115 lines** (0.37%). Deleting duplicate
samplers, raster execution and range-allocator plumbing is offset by nested
whole-frame scheduling, validation and regression coverage. Moving generators is
an ownership cleanup, not counted as deleting their implementation.

`cargo fmt` followed by the full `pnpm check` passed. Both firmware feature sets
passed release Clippy with warnings denied. Six collector contract tests passed,
including the requirement that source symbolization waits until UART capture ends.
Several profiling attempts had corrupt serial data and remain failed evidence
files; they were not repaired by ignoring bytes. Deferred symbolization eliminates
one host-side receive stall, but does not establish the cause of every corrupt
byte. The Windows collector also requests a 64-KiB receive buffer through pyserial
to tolerate scheduling stalls; the driver may ignore this request, and it is not a
proof that the USB/UART link is fault-free.

The first final-image Wi-Fi run (`2026-09-06-audit-i2s.txt`) completed three
uploads, but timed out awaiting the corrupted-body rejection response. It is
incomplete evidence, not an accepted playback capture.

## ESP32 evidence

The final tested loader ELF is `firmware/dawn-profile/target/audit-loader-verified.elf`,
SHA256 `512f3c39d4e37b4815411c97ca7352ddc57916d33f0f62982b84fc30765ab67c`.
All files below are under `firmware/dawn-profile/results/`.

### Starter show with Wi-Fi and parallel I2S

`2026-09-06-audit-i2s-verified.txt` completed three 25,268-byte replacements in
0.406, 0.188 and 0.172 seconds. Decode/workspace construction took 11.2-11.9 ms.
Authorization, version, size, checksum, concurrent-upload and interrupted-body
checks passed; the retained sequence then matched all 200 requested frame CRCs,
with zero evaluation allocations. Radio/HTTP tasks may allocate independently.

The following 109 captured windows contain **13,080 frames at 120 Hz with zero
missed deadlines**, including the dense Spin regions. Ordinary evaluation is
1.83-1.84 ms, encoding about 2.48 ms, and overlapped total about 6.35 ms. The worst
window averages 4.462 ms evaluation; the maximum recorded complete frame is
7.939 ms, below the 8.333-ms deadline. Heap free ranges from 79,676 to 81,376 bytes
and finishes at 81,376; the capture does not show sustained heap growth. No LEDs
or oscilloscope were attached: this proves checksum
agreement and I2S DMA completion, not external waveform/voltage correctness.

The nearest pre-audit comparison is `2026-09-05-prepared-curve-crossing-run3.txt`:
ordinary evaluation about 1.85 ms, hottest window 4.388 ms, and no steady misses.
Thus representative ESP32 playback is broadly unchanged: a small ordinary gain,
a roughly 2% hotter worst window, and unchanged 120-Hz deadline success. This does
not reproduce the 12% desktop controller benchmark slowdown. That desktop test
uses the complete layout rather than the four-port fragment, on different hardware.

The pre-lock-fix run `2026-09-06-audit-i2s-retry.txt` had 50 misses in its first
captured window overlapping HTTP verification, then zero in the remaining 108
windows. Its evaluation maximum was only 2.154 ms in that first window, but total
latency reached 130.216 ms: the playback timer includes waiting for the mutex,
whereas the evaluation timer starts after acquiring it. The handler held the same
mutex across the awaited HTTP response write. Releasing it before that write
removed this stall in the final capture. Frame requests still compete for execution
and workspace ownership; this is not a guarantee of uninterrupted playback under
arbitrary diagnostic traffic.

### Controller-sized effect fixtures

Six controller-shaped fixtures were transferred over Wi-Fi and each evaluated at
32 host-selected times, repeated three times. All **576 frame CRCs matched**, with
zero evaluation allocations. These use the real codec, runtime and output patch.

| 200-pixel fixture | Median HTTP evaluation | Archive bytes |
| --- | ---: | ---: |
| Chase/pulse, 1 layer | 1.397 ms | 3,836 |
| Chase/pulse, 4 layers | 2.595 ms | 4,294 |
| Chase/pulse, 16 layers | 8.745 ms | 6,160 |
| Generated mark pulse | 1.163 ms | 19,574 |
| Generated mark pulse, edge fade | 2.184 ms | 19,574 |
| Generated mark chase | 32.241 ms | 6,070 |

The evidence files are `2026-09-06-audit-final-{ChasePulse1,ChasePulse4,ChasePulse16,
MarkPulse200,MarkPulseEdge200,MarkChase200}.txt`. HTTP evaluation runs on the radio
core, with interrupt/cache interference and competing output work. These are not
pure steady-state CPU timings, do not include encoding/wire time, and are not
directly comparable to the isolated-core PC profiler. The mark-chase stress case
is clearly compute-limited; 120 Hz is not promised for arbitrary effect stacks.

The accepted intermediate PC capture is `2026-09-06-audit-pc-drained.txt`, ELF
SHA256 `856adf9dd9298d108b3bb1d1182e98a6004b7147461fbb0a8cc6e2340735cfd7`,
preserved as `target/audit-pc-accepted.elf`. It completed all 60 windows across 15
fixtures, with matching checksums and no timed allocations. It predates the final
mark broadcast/hue changes. Subsequent final-image PC captures failed strict
serial validation; there is **no accepted final-image leaf profile**. The final
Wi-Fi measurements above are the available final-image effect timing evidence.

## Verification coverage

- `native_effect_parity` pins the native parameter ABI and compares native
  generator schedules and samples with reference DSL behavior.
- `native_project_parity` checks native versus DSL effects in actual project frames.
- `native_temporal` compares whole-frame Delay/Echo through nested native operators
  with forced scalar sampling, including mark pulses, edge fading, mark chases,
  and nonmonotonic time requests.
- `sampling_parity` exhaustively checks multiplication rounding and checks gradient
  steps/boundaries through bound and direct resource access.
- `bound_params_allocations`, `controller_allocations`, and `show_workspace`
  exercise prepared storage reuse, automation, array lifetimes and frame seeks.
- `sequence_wire` roundtrips real selected sequences, compares rendered output,
  and rejects corrupt, incompatible, over-budget and invalid target-cache data.
- `output_selection` checks fragmented outputs against full-sequence output;
  patch packing and the normal starter checksum gates cover the downstream output.
- `cargo fmt` then `pnpm check` cover the complete desktop workspace and frontend;
  firmware release Clippy covers `pc-profile` and `i2s-output` separately.
