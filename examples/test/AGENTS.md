# Repository Guidelines

This is a Dawn project. Keep authored project files in `project.dawn`, `displays/`, `layouts/`, `fixtures/`, `patches/`, `curves/`, `effects/`, and `sequences/`.

Use existing curves and effect scripts before adding new ones. Do not add generated files, compatibility shims, hidden fallbacks, or destructive cleanup. Never overwrite user-authored show files without inspecting them first.

Validate edits with Dawn tooling from the Dawn workspace, for example `cargo run -p dawn-cli -- analyze <project-folder>`.
