fn main() {
    let result = if std::env::args().any(|arg| arg == "--check") {
        dawn_desktop::check_bindings()
    } else {
        dawn_desktop::export_bindings()
    };

    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
