fn main() {
    if let Err(error) = dawn_desktop::run() {
        eprintln!("failed to run Dawn desktop: {error}");
        std::process::exit(1);
    }
}
