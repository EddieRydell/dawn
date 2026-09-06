# Prepared sequence loading on ESP32

The laptop elaborates only the selected controller outputs, serializes the resulting
`PreparedSequence`, and uploads that archive over HTTP. The ESP32 validates and
decodes the same type, creates its reusable workspace once, then evaluates it at a
requested time. Parsing, compilation, generator expansion, and elaboration remain
on the laptop. With the optional `rmt-output` feature, the same firmware also
evaluates continuously and drives four parallel WS2812 outputs.

## Transport

The firmware uses `picoserve` 0.20 with Embassy networking. There is no Dawn
chunking, acknowledgement, request parser, or raw TCP command protocol.

- `PUT /sequence` streams one binary archive into a bounded buffer. A successful
  decode atomically replaces the active sequence. Invalid or interrupted uploads
  leave the previous sequence active. Only one upload may allocate and decode at
  a time; a concurrent upload gets HTTP 409 while the other server worker remains
  available.
- `POST /frame` accepts one little-endian `u32` sample time and returns the frame
  CRC, evaluation time, and allocation counters. This endpoint is verification
  scaffolding; scheduled playback will call the runtime directly.
- Both endpoints require the per-boot `X-Dawn-Token` header.

`picoserve` supplies HTTP parsing, method/path dispatch, body framing and IO
timeouts. Its default three-second whole-request timeout was too short for the
32 KiB device limit on variable Wi-Fi, so only an authenticated sequence body gets
a 15-second timeout. Two library-managed workers allow keep-alive without letting
one client own the only socket, and let a fresh connection proceed while an
interrupted body expires. Persistent-request and response-write deadlines are
five seconds.

USB serial is used only to provision Wi-Fi and return the DHCP address and random
128-bit HTTP token. The host initiates this exchange with `P`; it no longer depends
on a one-shot boot greeting. This fixes the observed false startup timeout, where
the board was alive but the first characters of its unsolicited greeting were
damaged. Boot logging may still be corrupted and is not part of the protocol.

Credentials, token, and the active sequence are currently RAM-only, so reset still
requires provisioning and upload. Persistent credential/sequence storage and a
real provisioning UI remain product work. HTTP and the token are plaintext and are
appropriate only on a trusted LAN; this is not an Internet-facing service.

## Archive and memory

`crates/dawn-runtime/src/wire.rs` owns `encode_sequence`, `decode_sequence`,
`LoadLimits`, and `LoadError`. Runtime and codec remain `no_std + alloc`. The
16-byte header contains `DAWN`, a `u32` version, payload length, and CRC32. The
payload is a 32-bit, little-endian rkyv archive validated before deserialization.
CRC detects corruption, not authenticity.

Firmware limits are currently 32 KiB of payload, 1,600 pixels, 128 graph nodes,
and a 96 KiB estimated workspace. Loading allocates; successful frame evaluation
does not. The normal loader's admission formula leaves 16 KiB for other tasks and
reserves eight times the archive size for decoding headroom. The RMT build uses
twice the archive size because its four one-time pulse-buffer allocations reduce
the free heap substantially. These are admission policies, not proofs against OOM.

The current layered starter archive is 7,856 bytes for 113 pixels, seven effects,
and one bytecode program. It contains effects and prepared sequence data, not
precomputed RGB frames.

The classic ESP32 has 520 KiB of on-chip SRAM, but it is split among executable
data, static data, stacks, caches and heap; it is not a 520 KiB application heap.
This firmware explicitly provides a 192 KiB allocator. On the tested board the
dual-core Wi-Fi/RMT build had 73,108 heap bytes free before loading and 62,208
after loading the starter sequence. The four RMT pulse buffers account for
76,832 bytes and the application-core stack reserves another 4,096 static bytes.

A representative synthetic 100-effect fragment, made from the starter's Spin
and Pulse effects over the same 113 pixels with one shared program, encoded to
26,014 bytes. Its decoded sequence, workspace and output retained 43,348 bytes,
and decoding peaked at 57,232 bytes in addition to the resident archive. This is
not a universal per-effect multiplier: parameters, automations, targets and unique
programs all affect the size. It also means that the current RAM-buffered loader
cannot safely load that sample while four RMT outputs are allocated, even though
the final decoded state would fit. Flash-backed upload staging or decoding without
retaining the whole archive is the next memory fix.

## Build and verify

Export the selected first controller output from the repository root:

```powershell
cargo run -p dawn-elaboration --example export_sequence -- examples/starter firmware/dawn-profile/target/loaded-sequence.dawnseq
```

Then run from `firmware/dawn-profile`:

```powershell
. C:/Users/eddie/export-esp.ps1
cargo +esp build --release --features loader --bin loader --locked
espflash flash --port COM4 --baud 38400 --chip esp32 --non-interactive --flash-size 4mb --flash-mode dio --flash-freq 40mhz target/xtensa-esp32-none-elf/release/loader
uvx --from esptool python upload.py target/loaded-sequence.dawnseq --windows-profile YOUR_PROFILE --uploads 3 --exercise-rejections --log results/http-load.txt
```

Use `--features rmt-output` instead of `loader` to run continuous 120 Hz playback
and four concurrent 200-pixel outputs on GPIO13, GPIO18, GPIO21 and GPIO25. This
starts a second Embassy executor: Wi-Fi/HTTP stays on core 0, while sequence
evaluation, color preparation and RMT interrupt servicing run on core 1. Dawn's
`atomic` feature is enabled only for this multicore build so the immutable shared
sequence and its workspace can legally cross the core boundary; frame evaluation
does not clone or drop those shared references.

Alternatively use `--ssid YOUR_2_4_GHZ_SSID`; the tool prompts for its password
without echo. On English Windows, `--windows-profile` reads the selected saved
personal-network profile in memory. Passwords and tokens are never logged or
written to evidence files.

The accepted loader-only hardware run used loader ELF SHA256
`47020836d08efa737712aaf707e163532268563524c2b69191f6428d66bf14f1`
and payload SHA256
`89bb791bc2978904c4335f391c72a3a293be5d823de54f737aa4dbe3c21a8bef`.
Three HTTP uploads completed in 0.312, 0.094, and 0.094 seconds; decode, workspace,
and output construction took 7.201-11.441 ms. The device rejected an invalid token,
bad version, oversized declared payload, bad checksum, and interrupted body. It
also rejected a concurrent upload. It then evaluated 200 host-selected frames
correctly with zero evaluation allocations, proving that failed uploads retained
the active sequence. See the
[capture](../firmware/dawn-profile/results/2026-09-05-picoserve-http-final3.txt).

The `/frame` timing includes radio interrupts, HTTP handling immediately before
evaluation, and cold instruction-cache effects. Continuous Wi-Fi-free profiling
showed the SDK migration within -1% to +1.5% across all 15 fixtures, including
stacked chases and mark effects; use that result rather than HTTP request timing
as the current indication of steady playback performance. Historical measurements
and their limitations remain in `runtime_optimization_2026-09-05.md`.

The firmware currently pins the coordinated esp-rs SDK revision documented in
`firmware/dawn-profile/README.md`.

The accepted dual-core Wi-Fi/RMT run used ELF SHA256
`8d39dcd6d0627be488850bbed3942d808757901a1af179506f577eece9681210`.
The board reported core 1 for playback. Over idle 120-frame windows it averaged
about 0.57 ms for Dawn evaluation, 1.40 ms for four pulse preparations, 6.12 ms
for the concurrent RMT transfers, and 8.10 ms total. It usually missed 0-3 of 120
8.333 ms deadlines; the worst observed idle window missed 7. Before separating
the work across cores, typical windows missed 27-32 deadlines and averaged
8.50-8.58 ms total. Two hundred authenticated HTTP frame requests still returned
the expected checksums with zero evaluation allocations. No LEDs were connected,
so this verifies real GPIO/RMT transmission completion rather than light output or
waveform electrical quality. See the
[idle capture](../firmware/dawn-profile/results/2026-09-05-dual-core-playback.txt)
and [HTTP verification](../firmware/dawn-profile/results/2026-09-05-dual-core-http-200.txt).
