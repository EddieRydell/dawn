# Dawn profiling on the classic ESP32

This firmware runs the existing `dawn-runtime`, not a second interpreter. It is a
standalone Cargo workspace so its Xtensa toolchain, target configuration, board
dependencies and lockfile do not affect desktop builds.

Tested board: COM4, ESP32-D0WD-V3 revision 3.1, 40 MHz crystal, 4 MB flash.
The old application was replaced without a backup, as requested. No LED GPIOs
are configured. Wi-Fi and the second application CPU are not started.

## Build and capture (Windows)

The installed tools are `espup` 0.17.1, `espflash` 4.5.0, Xtensa Rust
1.97.0.0 (`rustc 1.97.0-nightly`, commit `8ea53bcd7`, LLVM 21.1.3), and
Espressif GCC 15.2.0. The firmware lockfile pins the board crates, including
`esp-hal` 1.1.2, `esp-alloc` 0.10.0 and `esp-println` 0.17.0.

From this directory:

```powershell
. C:/Users/eddie/export-esp.ps1
cargo +esp build --release --bin dawn-profile --locked
espflash flash --port COM4 --baud 57600 --chip esp32 --non-interactive --flash-size 4mb --flash-mode dio --flash-freq 40mhz target/xtensa-esp32-none-elf/release/dawn-profile
uvx --from esptool python capture.py target/xtensa-esp32-none-elf/release/dawn-profile --raw-output results/profile.txt
```

Flashing overwrites the application, bootloader and partition table. The capture
script only resets the flashed application and reads COM4; it does not flash.
Use a new output filename for each run; existing files are never overwritten.
Preserve the exact ELF before another build, since the capture records its hash.
It closes the port on completion/error and rejects duplicate measurements,
checksum mismatches, unexpected profiling records and incomplete runs. A serial
transfer checksum failure occurred at the default upload speed. Later 115200-baud
flashes also failed intermittently; 57600 is the currently tested upload setting,
but has still occasionally required identical-image retries. Capture UART remains
115200 baud. Both Windows/.NET and pyserial captures have encountered corrupt bytes;
changing collectors is not a proven fix. The checked-in collector uses pyserial
(provided by the esptool tool environment) and rejects corrupted captures.

## Interrupted-PC profiling

The optional `pc-profile` feature enables the timer API used by `pc_profile`.
It affects this firmware workspace, not the shared runtime's dependencies.

```powershell
. C:/Users/eddie/export-esp.ps1
cargo +esp build --release --features pc-profile --bin pc_profile --locked
espflash flash --port COM4 --baud 57600 --chip esp32 --non-interactive --flash-size 4mb --flash-mode dio --flash-freq 40mhz target/xtensa-esp32-none-elf/release/pc_profile
uvx --from esptool python capture_pc.py target/xtensa-esp32-none-elf/release/pc_profile --raw-output results/pc-profile.txt
uvx --from esptool python -m unittest test_capture_pc
```

This records interrupted instruction addresses, not call stacks. The collector
resolves leaf symbols using the supplied ELF, leaving unknown addresses explicit.
Preserve that ELF alongside the result: a later build can move symbols.

Each of 15 fixtures renders 200 pixels in four windows: sampling disabled, 997-us
sampling, 1999-us sampling, then disabled again. The 4096-entry PC buffer occupies
16 KiB; the profiler uses a separate 96-KiB heap. UART records are emitted after
timing and interrupts stop. Successful timed frames must not allocate, and the last
rendered frame in each window must match the host checksum. The normal harness
provides the broader first-frame and every-frame checksum coverage.
The mark fixtures include both disabled and nonzero MarkPulse edge fading, so
section-index arithmetic is actually exercised in `MarkPulseEdge200`.

Periodic samples can alias with the workload; the two periods help expose this but
do not eliminate sampling bias. Windows finish the current 32-frame show cycle
after at least two seconds, preserving an equal mixture of show times. The collector
rejects partial cycles. Earlier captures used partial cycles and are not directly
equivalent for effects whose cost varies across the window. These are leaf
attributions, not inclusive stack percentages or hardware cache-miss counters.
`SYMBOL` records use the linker symbol table; `SOURCE` records resolve inlined leaf
functions and source lines through the same ELF's DWARF information. Unknown source
locations remain unknown. Resolution runs on the host outside device timing, batching
unique addresses through `addr2line`; it does not reconstruct dynamic call stacks.
The runtime evaluator and float-division routine/literals are placed in IRAM by
`rwtext_hook.x`; code placement materially affected earlier profiles. Check the
paired disabled windows before interpreting sampling overhead or cross-image gains.

Both collectors reject corrupt/incomplete captures. A partial raw file left after
an error is evidence of a failed attempt, not a valid result; never repair it by
silently dropping bytes. See `../../docs/runtime_optimization_2026-09-05.md` for
accepted captures, exact image hashes and current measurement limitations.

The root desktop toolchain remains Rust 1.96.0. Do not run desktop builds with
`+esp`. To check the firmware itself:

```powershell
cargo +1.96.0 fmt --check
cargo +esp clippy --release --features pc-profile --bins --locked -- -D warnings
```

## Workloads and measurement boundaries

The four DSL sources and parameter sets are shared with the desktop Criterion
benchmark in `crates/dawn-language/benches/fixtures/mod.rs`. `build.rs` uses the
real host compiler and generates Rust constructors for its bytecode/resources
in Cargo's `OUT_DIR`. This is benchmark fixture emission, **not** Dawn's future
serialized prepared-show format. Unsupported resource values fail the build.

For each effect, the firmware measures 200, 400, 800 and 1600 pixels. Four added
fixtures, UniformFade, PixelRamp, ArrayRamp and DynamicArray, also measure 4 and
16 fully overlapping layers. ArrayRamp produces the same colors as PixelRamp
through fixed-index array syntax, now lowered to scalar operations by the
compiler. DynamicArray uses runtime-selected indices over a fixed-size array,
now lowered to direct value-slot selection. The collector expects 168 measurements total:

- `uniform_full` / `uniform_reuse`: UniformFade sampled through an identity operator,
  with conservative metadata forcing per-pixel effect evaluation or the real compiler
  metadata allowing uniform-result reuse. Both match the same host golden frames.

- `vm`: direct bytecode sampling into a preallocated GRB buffer. This includes
  constructing each pixel's context and writing its three output bytes.
- `show`: the real `PreparedSequence::evaluate` path (historical benchmark label), with one effect, one RGB
  element, one layer, a layer/output signal graph, and an RGB-to-GRB patch.
  `workload.rs` constructs this small known prepared fixture at startup. It does
  not run the production authoring/elaboration pipeline on the ESP32.
- `gamma_raw` / `gamma_lookup`: PixelRamp with gamma 2.2 and GRB packing,
  using the original component filters or a host-built 256-byte lookup table.
  Both paths are checked against the same host-generated checksums. Their
  setup and retained heap include the component workspaces or lookup table.
- `operator_full` / `operator_reuse`: PixelRamp sampled through a DSL signal
  operator with a time-dependent sine gain. The same program runs with its
  uniform prefix repeated per pixel or reused within the frame. Both paths
  must match the same host-generated checksums.
- `native_automation`: native Pulse with a curve automation clip, exercising
  curve updates while the previous frame's native sample holds a curve reference.
  Host checks compare reused and fresh workspaces before emitting golden checksums.
- `empty_automation`: the same native fixture with empty parameter and automation
  curves, exercising the single fallback point and its preallocated buffers.
- `mixed_native`: a native Invert between two DSL identity operators, sharing
  VM depth slots as in elaboration. Host goldens check inverted direct-VM colors.
- `ArrayLifetimes` runs the existing nested-array lifetime fixture through direct
  VM and single-layer prepared evaluation. Its build asserts nonzero array storage,
  so this covers the general arena rather than only scalarized array syntax.
- `temporal_grouped` / `temporal_alternating`: four reads of the same two
  upstream times, grouped or interleaved, using the desktop benchmark sources.
  Compiler cleanup removes repeated reads; both variants must match host checksums.
- `nested2` / `nested4` / `nested8`: chains of the sine-gain DSL operator,
  sharing bytecode with one preallocated VM register workspace per depth.
  Host checks compare reused and fresh workspaces. These measure time and heap,
  not stack high-water usage or a safe maximum nesting depth.
- `nested4_full` / `nested8_full`: the same nested chains with prefix reuse
  disabled by setting bytecode pixel entry to zero, for a same-image comparison.
  Both modes must match the same host checksums.

The show has an eight-second duration. The measured 32-frame window starts at
three seconds and advances by 8333 microseconds/frame. These are varied show
times, not repeated evaluation of one frozen frame. They are not a whole-show
worst-case workload, nor the exact time/context choices of the desktop suite.
The added layered fixtures use identical inputs and max composition, and their
single-layer graph uses elaboration's output alias. The paired operator fixture
samples its upstream signal at the current time. Native curve automation and
two-time signal reads and bounded nested operators are covered separately.
ArrayLifetimes covers bounded nested-array construction and alias retention, not
exhaustive array stress. Network reception and physical pin output are not included.

CPU clock is configured to 240 MHz and reported from the HAL. Release uses
`opt-level=3`, fat LTO and one codegen unit. Flash access uses 40 MHz DIO;
the 160 KiB configured heap is internal RAM, not PSRAM. One warmup frame precedes
the 32 timed frames. The VM interpreter and specialized show evaluator are now
placed in instruction RAM through `rwtext_hook.x`; remaining code executes from
flash. UART printing is outside the timing windows. Host checksum
generation independently compares the prepared-show output with direct VM
output. Both device paths then check every measured frame against those host
checksums, outside the timed regions.

`setup_us` measures on-device fixture/resource construction, parameter binding,
buffer creation and (for shows) workspace preparation. It is not host DSL
compilation/elaboration time. First-frame time and allocation calls are reported
separately. `retained_bytes` is allocator-reported used heap above the empty
baseline after warmup. `peak_requested_bytes` comes from esp-alloc hooks and
counts requested live payload bytes during setup/evaluation, excluding allocator
bookkeeping; it is not total RAM or stack high-water usage. Hook counters use
32-bit atomics and do not print or allocate. There is no replacement allocator.

Times are microseconds; mean FPS is `1_000_000 / mean_us`, rounded down in serial
output. The 32-sample p95/max describe only the observed window, not a deadline
guarantee. The HAL's hardware time read occurs per frame, not per VM instruction.

## Results

See [the measured baseline](../../docs/esp32_profiling.md). The firmware remains
installed; resetting the board reruns the suite. The capture process releases
COM4 when finished.

See the [active optimization checkpoint](../../docs/runtime_optimization_2026-09-04.md)
for newer results, intentional math changes, allocation limitations and remaining
work. The baseline is historical, not the current firmware's performance.
