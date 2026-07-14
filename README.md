# Dawn

Dawn is a desktop workbench for authoring programmable light shows as source-controlled projects. It combines an IDE-style editor, project validation, timeline-oriented sequencing, real-time preview rendering, and export tooling for show files.

The project is built as a Rust workspace with a Tauri desktop shell and a React/TypeScript frontend. The core model, Dawn document loading, effect DSL, and renderer live in Rust so project validation and frame rendering share the same typed domain model.

## Why This Exists

Lighting tools often split creative sequencing from the source data that makes a show maintainable. Dawn treats a light show like a real project: logical element trees, preview props, fixture profiles, typed patch graphs, controllers, effects, sequences, and audio references are stored as Dawn source documents, checked together, and edited through text and GUI workflows.

That makes the project useful as a technical showcase for:

- A typed Rust domain model for a non-trivial creative tool.
- A custom effect DSL with parsing, type checking, compilation, and VM execution.
- A real-time renderer for pixel-based lighting sequences.
- A desktop application architecture that keeps frontend UI state synchronized with Rust-owned project state.
- Practical editor features such as diagnostics, project trees, generated TypeScript bindings, and example projects.

## Features

- Open and validate Dawn project files.
- Edit project documents in a CodeMirror-based desktop editor.
- Work with elements, fixture profiles, preview links, typed patches, controllers, effects, curves, sequences, and audio-backed timelines.
- Render one shared logical/controller frame through the Rust runtime.
- Preview effect rasters and sequence output in the desktop UI.
- Transmit live E1.31 or Art-Net output with blackout and stream lifecycle handling.
- Generate TypeScript bindings from Rust command and data types.
- Benchmark effect VM and render performance with Criterion.

## Tech Stack

- Rust 2024 workspace
- Tauri 2 desktop runtime
- React, TypeScript, and Vite frontend
- CodeMirror editor integration
- wgpu-backed rendering infrastructure
- Criterion benchmarks
- pnpm workspace tooling

## Repository Layout

```text
apps/desktop/                 Tauri desktop app
apps/desktop/src/             Rust desktop service, app state, commands, persistence
apps/desktop/src/state/       Desktop audio, workspace, GUI edit, project, render, and filesystem workflows
apps/desktop/src/gui/         Typed GUI projection, edit, selection, and domain-conversion modules
apps/desktop/src/state_tasks.rs Background save/render scheduling and GUI history
apps/desktop/src/gui_geometry.rs Read-only preview-prop geometry projection
apps/desktop/frontend/        React/TypeScript frontend
apps/desktop/frontend/src/ui/gui/sequence/sequenceWaveform.ts  Timeline waveform cache/rendering
crates/dawn-language/         Core Dawn model, sequence types, effect DSL, compiler, VM
crates/dawn-project-io/       Dawn project loading, diagnostics, source ownership, save/export
crates/dawn-project-io/src/loader/  Project loading, import resolution, and document parsing
crates/dawn-project-io/src/serialization/  Domain-specific Dawn document serialization
crates/dawn-runtime/          Prepared sequence and effect rendering
crates/dawn-output/           E1.31 and Art-Net socket/codec lifecycle
examples/                     Example Dawn projects and props
docs/                         Performance and regression tracking notes
tools/                        Repository tooling
```

## Getting Started

### Prerequisites

Install:

- Rust toolchain from `rust-toolchain.toml`
- Node.js `>=26.4.0`
- pnpm `>=11.9.0`
- Tauri 2 system dependencies for your operating system

### Install Dependencies

```bash
pnpm install
```

### Run The Desktop App

```bash
pnpm tauri dev
```

This starts the Vite frontend through Tauri and opens the Dawn desktop app.

### Try An Example Project

After the app opens, load one of the example project entrypoints:

```text
examples/starter/project.dawn
examples/thirty-output-controller/project.dawn
examples/christmas-house/project.dawn
```

`examples/starter` is the smallest project to explore first. `examples/thirty-output-controller` is a larger show used for realistic render and performance work.

## Development Commands

```bash
pnpm generate:bindings
```

Regenerates TypeScript bindings from the Rust desktop API.

```bash
pnpm check
```

Runs generated bindings, frontend type checking, linting, dead-export analysis, frontend tests/build, and the Rust format, check, test, and Clippy gates.

```bash
cargo fmt
```

Formats the Rust workspace.

```bash
pnpm bench:effect-vm:quick
```

Runs a quick Criterion smoke pass for the effect VM and renderer benchmarks.

```bash
pnpm bench:effect-vm
```

Runs the full Criterion benchmark set.

## How A Dawn Project Works

A Dawn project starts at a `project.dawn` entrypoint. That file imports the rest of the show definition: setups, element trees, preview layouts and props, fixture profiles, patch graphs, controllers, curves, effects, sequences, and assets.

Project IO loads reachable source files, validates imports and references, tracks source locations for diagnostics, compiles DSL definitions, and builds the authoritative typed `DawnProject`. `SourceProject` retains document ownership, import, original-source, and asset metadata; it is not a second editable project model. GUI commands make one private mutable candidate from the current immutable project snapshot; accepted snapshots are shared by state, history, save, and render work. Project IO serializes typed state directly without reparsing or synchronizing a YAML model.

At render time, `dawn-runtime` resolves element selections in tree order, composes color effects, evaluates typed control clips, applies explicit fixture behavior rules, and evaluates the prepared patch graph into exact controller-port buffers. Preview and live output consume that same `RenderedShowFrame`; neither reinterprets colors, fixture channels, or patch ordering. Live output is opt-in for each application run and fails closed by blacking out active ports and terminating E1.31 streams.

## Status

Dawn is an active prototype. The codebase emphasizes fast iteration, typed state, explicit validation, and a single project model shared by the editor, GUI workflows, and renderer.
