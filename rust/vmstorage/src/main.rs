use std::{path::PathBuf, process};

use clap::Parser;
use logstorage::{StorageConfig, VlStorage};

/// vmstorage entrypoint backed by the shared `logstorage` crate.
#[derive(Debug, Parser)]
#[command(author, version, about = "Rust scaffold for VictoriaLogs vmstorage", long_about = None)]
struct Args {
    /// Directory where data will be stored.
    #[arg(long, default_value = "victoria-logs-data")]
    storage_data_path: PathBuf,

    /// Run in read-only mode (reject writes).
    #[arg(long, default_value_t = false)]
    read_only: bool,
}

fn main() {
    let args = Args::parse();

    let mut cfg = StorageConfig::default();
    cfg.storage_data_path = args.storage_data_path;
    cfg.read_only = args.read_only;

    if let Err(err) = run(cfg) {
        eprintln!("vmstorage failed to start: {err}");
        process::exit(1);
    }
}

fn run(cfg: StorageConfig) -> logstorage::Result<()> {
    let _storage = VlStorage::new_local(cfg)?;
    // At this stage we only initialize the storage and keep the process alive.
    println!(
        "vmstorage initialized in local mode. Use logstorage::handle_internal_request to serve HTTP."
    );

    // Block forever; real implementation would wire HTTP server here.
    loop {
        std::thread::park();
    }
}
