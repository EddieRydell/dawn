use std::path::Path;

use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

pub fn init_file_logging(path: &Path) -> Result<(), String> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    tracing_subscriber::registry()
        .with(EnvFilter::new("deprecated_dawn_backend=info"))
        .with(fmt::layer().with_writer(file).with_ansi(false))
        .try_init()
        .map_err(|error| error.to_string())
}
