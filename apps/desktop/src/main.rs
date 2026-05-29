#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

fn main() {
    if let Err(error) = dawn_desktop::run() {
        eprintln!("failed to run Dawn desktop: {error}");
        std::process::exit(1);
    }
}
