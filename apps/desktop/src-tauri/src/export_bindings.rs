use std::path::PathBuf;

fn main() {
    let mut args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let check = args.first().is_some_and(|arg| arg == "--check");
    if check {
        args.remove(0);
    }
    let output = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("src/generated/bindings.ts"));

    let result = if check {
        check_bindings(output)
    } else {
        dawn_desktop_lib::export_bindings(output)
    };

    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn check_bindings(output: PathBuf) -> Result<(), String> {
    let temp = std::env::temp_dir().join("dawn-bindings-check.ts");
    dawn_desktop_lib::export_bindings(&temp)?;
    let expected = std::fs::read_to_string(&temp).map_err(|error| error.to_string())?;
    let actual = std::fs::read_to_string(&output).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(temp);
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "{} is stale; run `pnpm --filter @dawn/desktop bindings`",
            output.display()
        ))
    }
}
