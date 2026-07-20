fn main() {
    if let Err(error) = dawn_cli::run(dawn_cli::Cli::parse_args()) {
        eprintln!("dawn: {error}");
        std::process::exit(1);
    }
}
