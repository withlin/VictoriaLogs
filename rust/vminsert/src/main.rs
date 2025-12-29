use clap::Parser;
use logstorage::{LogRow, VlStorage};

/// vminsert stub that reuses the shared `logstorage` crate.
#[derive(Debug, Parser)]
#[command(author, version, about = "Rust scaffold for VictoriaLogs vminsert", long_about = None)]
struct Args {
    /// Target storage nodes; when empty, uses embedded local storage for dev/testing.
    #[arg(long = "storage-node")]
    storage_nodes: Vec<String>,
}

fn main() {
    let args = Args::parse();

    if args.storage_nodes.is_empty() {
        println!("No storage nodes provided. Using in-process storage (dev mode).");
        let storage = VlStorage::new_local(Default::default())
            .expect("failed to init local storage for vminsert stub");

        // Example ingest path for smoke-testing.
        let rows = vec![LogRow::new(0, Vec::new())];
        if let Err(err) = storage.add_rows(rows) {
            eprintln!("ingest failed: {err}");
        }
    } else {
        println!("vminsert would forward data to: {:?}", args.storage_nodes);
    }
}
