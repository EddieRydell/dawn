# Effect VM Performance

Dawn uses Criterion for Effect DSL VM and real-project render benchmarks. Timing deltas are
advisory; benchmark assertions fail only when the VM or renderer changes output.

## Commands

Run the full Effect VM benchmark set:

```powershell
pnpm bench:effect-vm
```

Run a faster local smoke pass:

```powershell
pnpm bench:effect-vm:quick
```

The quick pass only checks that every benchmark runs. Its reduced sample count is not reliable
enough to classify small performance changes.

Save a baseline before optimization work:

```powershell
pnpm bench:effect-vm:save
```

Compare the current working tree against the saved baseline:

```powershell
pnpm bench:effect-vm:compare
```

Criterion stores baselines and reports under `target/criterion`; do not commit them.

The full harness warms each workload for eight seconds, measures for ten seconds, and treats changes
below five percent as environmental noise. On Windows it also pins the benchmark thread to a fixed
nonzero logical CPU to avoid migrations between unlike cores. The pnpm full/save/compare commands
also launch one quick untimed pass first. This avoids treating one-time work on a newly linked
benchmark executable as a runtime regression.

## Focused Runs

Run one VM benchmark by name:

```powershell
cargo bench -p dawn-language --bench effect_vm_bench -- dsl_effect_suite
```

Run the representative render batch:

```powershell
cargo bench -p dawn-runtime --bench render_bench -- render_representative_frames
```

## Coverage

`crates/dawn-language/benches/effect_vm_bench.rs` measures direct VM paths through the public Effect
DSL APIs:

- `sample_bound`

The effect suite evaluates four representative DSL effects across 512 pixel contexts each, and the
operator measurement evaluates 512 signal samples. Together they cover curves, branches, section
position, smoothstep, enum comparisons, seeded random paths, trigonometry, HSV color generation,
and Signal input sampling. Sub-microsecond single-call benchmarks, native helpers unchanged by the
VM, and allocation-heavy generator timing were removed because power-state and allocator noise
repeatedly produced false regressions on unchanged code.

`crates/dawn-runtime/benches/render_bench.rs` measures the real
`examples/starter/project.dawn` renderer:

- renderer preparation
- one batch of seven representative sparse and dense frames
- warmed and cold 60-frame playback batches
- warmed controller-output batches through patch evaluation and port bytes
- frame checksums and active effect counts before timing begins

Update committed checksums only when a renderer or DSL behavior change is intentional.
