fn main() {
    let check = std::env::args().skip(1).any(|argument| argument == "--check");
    let result = if check {
        dawn_desktop::check_bindings()
    } else {
        dawn_desktop::export_bindings()
    };

    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
