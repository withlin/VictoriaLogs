use clap::Parser;
use logstorage::{LogSqlEngine, Query, VlStorage};

/// vmselect stub using the shared `logstorage` crate.
#[derive(Debug, Parser)]
#[command(author, version, about = "Rust scaffold for VictoriaLogs vmselect", long_about = None)]
struct Args {
    /// Target storage nodes for querying; empty means use local storage for dev.
    #[arg(long = "storage-node")]
    storage_nodes: Vec<String>,
}

fn main() {
    let args = Args::parse();

    if args.storage_nodes.is_empty() {
        println!("No storage nodes provided. Using in-process storage (dev mode).");
        let storage = VlStorage::new_local(Default::default())
            .expect("failed to init local storage for vmselect stub");
        let engine = LogSqlEngine::new(&storage);
        let results = engine.query(&Query::default()).unwrap_or_default();
        println!("vmselect query returned {} rows (stub)", results.len());
    } else {
        println!(
            "vmselect would query remote nodes: {:?}",
            args.storage_nodes
        );
    }
}
