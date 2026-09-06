# Dawn: measured classic ESP32 baseline

> Historical baseline. Its terminology and measurements describe the original
> profiling image; use [the execution audit](execution_audit_2026-09-06.md) for
> current runtime and ESP32 claims.

This is the original baseline. See the [execution audit](execution_audit_2026-09-06.md)
for newer math/compiler/IRAM results and the remaining overhaul work.

Measured September 4, 2026 (local time) on the user's COM4 board:
ESP32-D0WD-V3 revision 3.1, one application core at 240 MHz, 4 MB flash,
40 MHz DIO flash access, internal-RAM heap, Wi-Fi off, no LEDs connected.
This is classic ESP32/LX6 data, not ESP32-S3 data or desktop extrapolation.

The profiling firmware was built, flashed and run to completion. The old
application was overwritten without backup. No LED GPIOs were configured.
An independent reset/capture using pyserial also completed all 32 measurement
records and verified all host checksums. A separate corrupted serial capture
was rejected/discarded; it is not used as evidence of runtime instability.

## Prepared-show throughput

Each cell is computed FPS (`1,000,000 / mean frame microseconds`), rounded to
one decimal. Each workload has one active effect, one element, one layer,
and the production sequence evaluation and RGB-to-GRB patch path. There is no
physical transmission or radio overhead in these numbers.

| Pixels | RGB channels | ScanSweep | ImpactBurst | SparkleComet | ShimmerField |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 200 | 600 | 113.9 | 112.4 | 30.2 | 45.3 |
| 400 | 1200 | 57.5 | 56.6 | 15.2 | 22.5 |
| 800 | 2400 | 28.9 | 28.4 | 7.6 | 11.3 |
| 1600 | 4800 | 14.5 | 14.2 | 3.8 | 5.6 |

For 200 pixels, average full-show times are respectively 8.782, 8.895,
33.064 and 22.084 ms. Corresponding worst observed frame times are
8.826, 8.948, 33.755 and 22.666 ms. All exceed the 8.333 ms budget for
120 FPS. These are observed-window maxima, not worst-case execution bounds.

Direct VM sampling, including context construction and GRB buffer writes,
averages 6.144, 6.315, 28.317 and 19.209 ms for 200 pixels respectively.
Thus the first two effects are near a 200-pixel strand's wire ceiling at the
direct-VM level, but the complete show path is already compute-limited in
this build. Parallel LED outputs do not remove that CPU work.

This supersedes the earlier illustrative 5/15/40-microsecond-per-pixel
capacity table as evidence for this particular board/build/workload. It does
not establish a fundamental ESP32 limit or predict an optimized implementation.

## Correctness and memory

- All 1,024 frame checksum comparisons per complete run pass: four effects,
  four pixel counts, two evaluation paths, 32 frames each. Host reference
  generation separately checks prepared-show bytes against direct VM output.
- Every measured warmed case reports zero heap allocation calls. The first
  direct-VM frame allocates four or five register buffers; the first show
  frame reports zero allocation calls after workspace preparation.
- The configured heap is 163,840 bytes. At 200 pixels, the prepared-show
  cases retain 11,928-13,352 bytes. At 1600 pixels they retain 74,928-76,352
  bytes, leaving at least 87,488 bytes free in that configured heap.
- Retained heap includes the loaded case's bytecode, bound resources, prepared
  fixture, workspace and output buffer. It does not include all four effects
  simultaneously. It excludes static memory, stack and Wi-Fi buffers.
- The hook-measured peak requested payload for the 1600-pixel show cases is
  74,885-76,319 bytes, excluding allocator bookkeeping. This is not total RAM
  or stack high-water usage. Actual used-heap measurements above include the
  allocator's accounting overhead.
- Case teardown returns used heap to the baseline every time; the final
  reported free heap is again 163,840 bytes.
- Fixture construction/binding/workspace setup takes roughly 3.8-6.5 ms in
  this run. That is device setup, not host compilation/elaboration time.

Calculated-array allocation is still unfixed and not exercised by these four
workloads. These results do not prove universal runtime allocation freedom.
The vendored offset allocator is not yet used by the VM and cannot account
for a performance improvement here.

## Build and measurement sensitivity

An earlier firmware variant, before the added memory/first-frame reporting,
measured 13.522 ms direct VM and 36.560 ms show time for 200-pixel ScanSweep.
The later reporting variant measured 6.144 and 8.782 ms with no edits to the
Dawn runtime. Do not describe that change as a VM optimization: the firmware
code changed, and the cause of the large cross-build timing difference has
not been isolated. The complete later run was reproduced after reset.

Release disassembly confirms hardware `add.s`/`mul.s` instructions and calls
to the software single-precision division helper `__divsf3`; this is not a
build where all floating-point operations accidentally became software-only.
The emitted `Vm::run` function is 32,492 bytes, and the specialized show
evaluator is 17,735 bytes. ESP32 has 32 KB of cache per CPU. This makes code
placement/cache behavior a concrete investigation candidate, not a proven
cause or a measured breakdown of CPU time. See the
[Espressif technical reference manual](https://documentation.espressif.com/esp32_technical_reference_manual_en.pdf).

The successfully flashed reporting ELF had SHA-256
`ea41f0f464bddb7050e856943245f1cdf1e2fae82ff72f45439bc8d7a011756e`.
The generated application image is 392,320 bytes (9.5% of the default factory
partition). A subsequent rustfmt-only change altered only 65 image bytes:
the 32-byte embedded ELF hash, one checksum byte and the trailing 32-byte
image digest. All other image bytes, including executable code and workload
data, match. The before/after images are local Cargo build artifacts under
`firmware/esp32/target/`.

## What this does not cover

There is no networking, LED waveform output, multi-core scheduling, active
automation, layered effect composition, or serialized show loader in this
firmware. The prepared fixture is intentionally constructed at startup; the
compiler runs only on the PC. No physical-output test was performed because
no LEDs are connected. Wi-Fi interference remains unmeasured; radio-on
performance and memory headroom must be measured independently.

Desktop Criterion remains the desktop benchmark system. The embedded harness
uses hardware timing because Criterion's host execution environment cannot
run on this bare-metal target. Shared fixture extraction changes no effect
parameters or desktop sampling behavior. The root `pnpm check`, desktop
Criterion quick suite, firmware release build, firmware clippy with warnings
denied, and firmware formatting check pass. No production runtime source was
changed for profiling.

Source growth is 568 firmware/build-script Rust lines plus a net eight Rust
lines for sharing desktop fixtures, excluding documentation, capture tooling
and lockfiles. This adds board/profiling integration, not a duplicate runtime
or a completed runtime simplification.

See [firmware instructions](../firmware/esp32/README.md) and the
[complete baseline transcript](../firmware/esp32/results/esp32-v3.1-2026-09-04.txt).
