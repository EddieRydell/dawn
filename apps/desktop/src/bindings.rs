const BINDINGS_PATH: &str = "apps/desktop/frontend/src/bindings.ts";
const TYPED_ERROR_IMPL: &str = r#"async function typedError<T, E>(result: Promise<T>): Promise<{ status: "ok"; data: T } | { status: "error"; error: E }> {
    try {
        return { status: "ok", data: await result };
    } catch (error: unknown) {
        if (error instanceof Error) throw error;
        return { status: "error", error: error as E };
    }
}"#;

pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    crate::commands::register_commands(
        tauri_specta::Builder::<tauri::Wry>::new().typed_error_impl(TYPED_ERROR_IMPL),
    )
}

pub fn export_bindings() -> Result<(), Box<dyn std::error::Error>> {
    specta_builder().export(specta_typescript::Typescript::default(), BINDINGS_PATH)?;
    normalize_bindings_assertion(BINDINGS_PATH)?;
    Ok(())
}

pub fn check_bindings() -> Result<(), Box<dyn std::error::Error>> {
    let mut check_path = std::env::temp_dir();
    check_path.push(format!("dawn-bindings-check-{}.ts", std::process::id()));
    specta_builder().export(specta_typescript::Typescript::default(), &check_path)?;
    normalize_bindings_assertion(&check_path)?;

    let generated = std::fs::read_to_string(&check_path)?;
    std::fs::remove_file(&check_path)?;
    let current = std::fs::read_to_string(BINDINGS_PATH)?;
    if generated != current {
        return Err("generated bindings are stale; run `pnpm generate-bindings`".into());
    }
    Ok(())
}

fn normalize_bindings_assertion(
    path: impl AsRef<std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path)?;
    let normalized = source
        .replace("number | null", "number")
        .replace("(number)[]", "number[]")
        .replace(
            "const _assertTypedErrorFollowsContract: <T, E>(result: Promise<T>) => Promise<any> = typedError;",
            "void (typedError satisfies <T, E>(result: Promise<T>) => Promise<{ status: \"ok\"; data: T } | { status: \"error\"; error: E }>);",
        );
    if normalized != source {
        std::fs::write(path, normalized)?;
    }
    Ok(())
}
