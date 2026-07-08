# Dawn

Dawn is a desktop workbench for authoring programmable light shows as source-controlled projects. It combines an IDE-style editor, project validation, timeline-oriented sequencing, real-time preview rendering, and export tooling for show files.

The project is built as a Rust workspace with a Tauri desktop shell and a React/TypeScript frontend. The core model, Dawn document loading, effect DSL, and renderer live in Rust so project validation and frame rendering share the same typed domain model.

## Why This Exists

Lighting tools often split creative sequencing from the source data that makes a show maintainable. Dawn treats a light show like a real project: fixtures, layouts, patches, curves, effects, sequences, and audio references are stored as Dawn source documents, checked together, and edited through both text and GUI workflows.

That makes the project useful as a technical showcase for:

- A typed Rust domain model for a non-trivial creative tool.
- A custom effect DSL with parsing, type checking, compilation, and VM execution.
- A real-time renderer for pixel-based lighting sequences.
- A desktop application architecture that keeps frontend UI state synchronized with Rust-owned project state.
- Practical editor features such as diagnostics, project trees, generated TypeScript bindings, and example projects.

## Features

- Open and validate Dawn project files.
- Edit project documents in a CodeMirror-based desktop editor.
- Work with fixtures, layouts, patches, setups, effects, curves, sequences, and audio-backed timelines.
- Render lighting frames through the Rust runtime.
- Preview effect rasters and sequence output in the desktop UI.
- Export sequence data for downstream show tooling.
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
apps/desktop/frontend/        React/TypeScript frontend
crates/dawn-language/         Core Dawn model, sequence types, effect DSL, compiler, VM
crates/dawn-project-io/       Dawn project loading, validation, source maps, save/export
crates/dawn-runtime/          Prepared sequence and effect rendering
examples/                     Example Dawn projects and fixtures
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

`examples/starter` is the smallest project to explore first. `examples/thirty-output-controller` is a larger fixture used for realistic render and performance work.

## Development Commands

```bash
pnpm generate:bindings
```

Regenerates TypeScript bindings from the Rust desktop API.

```bash
pnpm check
```

Runs generated bindings, frontend type checking, linting, frontend build, Rust formatting checks, `cargo check`, and Clippy.

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

A Dawn project starts at a `project.dawn` entrypoint. That file imports the rest of the show definition: setups, fixture definitions, layouts, patches, curves, effects, sequences, and assets.

Project IO loads the reachable source files, validates references, tracks source locations for diagnostics, compiles effect definitions, and builds a typed `DawnProject`. The desktop app mutates that typed project state through Rust commands, then saves Dawn source documents through the IO layer.

At render time, `dawn-runtime` prepares the selected setup and sequence, resolves targets and fixture pixels, evaluates effect clips, applies automation, and composes layers through the sequence graph.

## Status

Dawn is an active prototype. The codebase emphasizes fast iteration, typed state, explicit validation, and a single project model shared by the editor, GUI workflows, and renderer.

