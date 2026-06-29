use specta_typescript::{Typescript, semantic};
use tauri_specta::Builder;

pub fn builder() -> Builder<tauri::Wry> {
    crate::commands::register(
        Builder::<tauri::Wry>::new()
            .semantic_types(semantic::Configuration::default().enable_lossless_floats()),
    )
}

pub fn export_typescript(
    path: impl AsRef<std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    builder().export(Typescript::default(), path)?;
    Ok(())
}
