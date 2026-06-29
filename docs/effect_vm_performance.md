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

Save a baseline before optimization work:

```powershell
pnpm bench:effect-vm:save
```

Compare the current working tree against the saved baseline:

```powershell
pnpm bench:effect-vm:compare
```

Criterion stores baselines and reports under `target/criterion`; do not commit them.

## Focused Runs

Run one VM benchmark by name:

```powershell
cargo bench -p dawn-language --bench effect_vm_bench -- scan_sweep
```

Run one render benchmark by frame:

```powershell
cargo bench -p dawn-runtime --bench render_bench -- render_frame_9504
```

## Coverage

`crates/dawn-language/benches/effect_vm_bench.rs` measures direct VM paths through the public Effect
DSL APIs:

- `compile_effects`
- `bind_params_cached`
- `sample_bound`
- `generate_bound`

The VM benches cover constant return overhead, curve color and float sampling, branch-heavy scan
logic, section position, smoothstep, enum comparisons, curve clamping, seeded random paths,
trigonometry, HSV color generation, dense mixed arithmetic, marks, target sections, `pick`, arrays,
loops, and `timeline.emit`.

`crates/dawn-runtime/benches/render_bench.rs` measures the real
`examples/thirty-output-controller/project.dawn` renderer:

- renderer preparation
- frames `144`, `2088`, `5904`, `9504`, `11520`, `19080`, and `25934`
- frame checksums and active effect counts

Update committed checksums only when a renderer or DSL behavior change is intentional.
