# Effect VM Performance

Baseline captured on 2026-06-12 before VM optimization edits.

## Baseline

Command:

```powershell
cargo bench -p dawn-runtime --bench render_bench -- --project examples/thirty-output-controller/project.dawn --frames 111,122,123,5760,9360 --iterations 2000 --warmup 100 --render-only
```

Results:

| Frame | p50 | p95 | Checksum |
| --- | ---: | ---: | --- |
| 111 | 0.002ms | 0.003ms | 70ff2d7b17783745 |
| 122 | 0.002ms | 0.002ms | 8101f63d0ad95168 |
| 123 | 2.858ms | 3.295ms | 34cba5bcdfc0ef01 |
| 5760 | 5.973ms | 7.643ms | c232ac4a02bfb53a |
| 9360 | 3.292ms | 3.984ms | c8854b88f2e20b00 |

Total: 10,000 rendered frames in 25,125.216ms.

Focused frame command:

```powershell
cargo bench -p dawn-runtime --bench render_bench -- --project examples/thirty-output-controller/project.dawn --frames 5760 --iterations 1500 --warmup 100 --render-only
```

Focused frame 5760 result: p50=6.108ms, p95=9.808ms, total=10,004.037ms, checksum=c232ac4a02bfb53a.

VTune command:

```powershell
& 'C:\Program Files (x86)\Intel\oneAPI\vtune\2025.3\bin64\vtune.exe' -collect hotspots -result-dir target\vtune-effect-vm-baseline-5760 -- cargo bench -p dawn-runtime --bench render_bench -- --project examples/thirty-output-controller/project.dawn --frames 5760 --iterations 1500 --warmup 100 --render-only
```

VTune benchmark result under profiling: frame 5760 p50=14.887ms, p95=20.074ms, total=21,509.176ms, checksum=c232ac4a02bfb53a.

Top VTune hotspots:

| Function | CPU Time | CPU % |
| --- | ---: | ---: |
| `dawn_language::effect_dsl::vm::Vm::run` | 6.996s | 34.8% |
| `RtlAllocateHeap` | 4.442s | 22.1% |
| `RtlFreeHeap` | 2.391s | 11.9% |
| `func@0x1400b39b0` | 0.688s | 3.4% |
| `func@0x1400b3b20` | 0.540s | 2.7% |

## Checkpoints

After typed arithmetic/mix dispatch, direct mark builtin slot reads, borrowed target item builtin args, and direct channel rounding:

```powershell
cargo bench -p dawn-runtime --bench render_bench -- --project examples/thirty-output-controller/project.dawn --frames 5760 --iterations 1500 --warmup 100 --render-only
```

Frame 5760: p50=5.648ms, p95=7.833ms, total=8,891.080ms, checksum=c232ac4a02bfb53a.

## Final

Focused frame command:

```powershell
cargo bench -p dawn-runtime --bench render_bench -- --project examples/thirty-output-controller/project.dawn --frames 5760 --iterations 1500 --warmup 100 --render-only
```

Final focused frame 5760 result after color channel math cleanup: p50=5.672ms, p95=9.664ms, total=9,547.643ms, checksum=c232ac4a02bfb53a. The earlier checkpoint remains the best focused-frame sample in this run; both focused samples preserve the checksum.

Full multi-frame command:

```powershell
cargo bench -p dawn-runtime --bench render_bench -- --project examples/thirty-output-controller/project.dawn --frames 111,122,123,5760,9360 --iterations 2000 --warmup 100 --render-only
```

Final full render-only sample:

| Frame | p50 | p95 | Checksum |
| --- | ---: | ---: | --- |
| 111 | 0.003ms | 0.004ms | 70ff2d7b17783745 |
| 122 | 0.002ms | 0.002ms | 8101f63d0ad95168 |
| 123 | 2.666ms | 4.337ms | 34cba5bcdfc0ef01 |
| 5760 | 5.773ms | 8.237ms | c232ac4a02bfb53a |
| 9360 | 3.026ms | 4.639ms | c8854b88f2e20b00 |

Total: 10,000 rendered frames in 24,396.247ms.

Final VTune command:

```powershell
& 'C:\Program Files (x86)\Intel\oneAPI\vtune\2025.3\bin64\vtune.exe' -collect hotspots -result-dir target\vtune-effect-vm-final-5760 -- cargo bench -p dawn-runtime --bench render_bench -- --project examples/thirty-output-controller/project.dawn --frames 5760 --iterations 1500 --warmup 100 --render-only
```

Final VTune benchmark result: frame 5760 p50=5.742ms, p95=8.784ms, total=9,270.029ms, checksum=c232ac4a02bfb53a.

Final top VTune hotspots:

| Function | CPU Time | CPU % |
| --- | ---: | ---: |
| `dawn_language::effect_dsl::vm::Vm::run` | 3.052s | 32.5% |
| `RtlAllocateHeap` | 2.396s | 25.6% |
| `RtlFreeHeap` | 1.041s | 11.1% |
| `func@0x1400b3580` | 0.514s | 5.5% |
| `RuntimeValue` drop | 0.218s | 2.3% |

Compared with baseline VTune, `Vm::run` CPU time decreased from 6.996s to 3.052s, `RtlAllocateHeap` from 4.442s to 2.396s, and `RtlFreeHeap` from 2.391s to 1.041s. `alloc::string::clone`, `Vm::value`, and `round` are no longer in the final top hotspots.

## Render-Focused Pass

Follow-up optimization focused on runtime rendering over generator preparation:

- `timeline.emit` now reads `start`, `duration`, and `target` directly from typed/borrowed slots instead of constructing owned `RuntimeValue`s for those fields.
- Ref equality now compares borrowed runtime refs directly, avoiding enum/string clones in equality.
- Enum param vs enum constant comparisons compile to a direct param/constant opcode, avoiding enum param and enum literal ref loads in hot sample branches.
- Prepared curve param indexing writes directly to float/color slots.
- A direct indexed register read/write experiment was tried and rejected: it worsened `Vm::run` in VTune (`2.694s -> 3.101s`) and was removed.

Focused render-only frame 5760 best warm checkpoint:

```powershell
cargo bench -p dawn-runtime --bench render_bench -- --project examples/thirty-output-controller/project.dawn --frames 5760 --iterations 1500 --warmup 100 --render-only
```

Result: p50=2.296ms, p95=3.914ms, total=3,727.542ms, checksum=c232ac4a02bfb53a.

Final full render-only sample:

| Frame | p50 | p95 | Checksum |
| --- | ---: | ---: | --- |
| 111 | 0.002ms | 0.004ms | 70ff2d7b17783745 |
| 122 | 0.002ms | 0.003ms | 8101f63d0ad95168 |
| 123 | 1.294ms | 2.323ms | 34cba5bcdfc0ef01 |
| 5760 | 2.400ms | 4.337ms | c232ac4a02bfb53a |
| 9360 | 1.615ms | 2.878ms | c8854b88f2e20b00 |

Total: 10,000 rendered frames in 12,115.715ms.

Final render-focused VTune:

```powershell
& 'C:\Program Files (x86)\Intel\oneAPI\vtune\2025.3\bin64\vtune.exe' -collect hotspots -result-dir target\vtune-effect-vm-render-final-5760 -- cargo bench -p dawn-runtime --bench render_bench -- --project examples/thirty-output-controller/project.dawn --frames 5760 --iterations 1500 --warmup 100 --render-only
```

VTune benchmark result: frame 5760 p50=2.445ms, p95=4.386ms, total=4,106.947ms, checksum=c232ac4a02bfb53a.

Top VTune hotspots:

| Function | CPU Time | CPU % |
| --- | ---: | ---: |
| `dawn_language::effect_dsl::vm::Vm::run` | 3.130s | 67.1% |
| `func@0x1400b3830` | 0.663s | 14.2% |
| `dawn_language::effect_dsl::vm::run_sample_effect` | 0.167s | 3.6% |
| `dawn_language::effect_dsl::vm::Vm::new` | 0.069s | 1.5% |
| `RuntimeValue` drop | 0.068s | 1.5% |

Allocator/free hotspots are no longer dominant in the render-focused VTune profile.

Prepare+render sample after the render-focused pass:

```powershell
cargo bench -p dawn-runtime --bench render_bench -- --project examples/thirty-output-controller/project.dawn --frames 111,122,123,5760,9360 --iterations 200 --warmup 20
```

Result: prepare p50=98.544ms, p95=135.000ms. Checksums unchanged. This pass should be considered primarily a rendering improvement, not a preparation improvement.
