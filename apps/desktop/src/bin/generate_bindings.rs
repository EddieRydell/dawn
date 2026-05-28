fn main() {
    if let Err(error) = dawn_desktop::export_bindings() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
