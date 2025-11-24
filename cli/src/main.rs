mod paths;
mod storage;

use clap::{Parser, Subcommand};
use pai_core::{Config, ListFilter, PaiError, SourceKind};
use std::path::PathBuf;
use storage::SqliteStorage;

/// Personal Activity Index - POSIX-style CLI for content aggregation
#[derive(Parser, Debug)]
#[command(name = "pai")]
#[command(version, about, long_about = None)]
struct Cli {
    /// Set configuration directory
    #[arg(short = 'C', value_name = "DIR", global = true)]
    config_dir: Option<PathBuf>,

    /// Path to SQLite database file
    #[arg(short = 'd', value_name = "PATH", global = true)]
    db_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Fetch and store content from configured sources
    Sync {
        /// Sync all configured sources (default)
        #[arg(short = 'a')]
        all: bool,

        /// Sync only a particular source kind
        #[arg(short = 'k', value_name = "KIND")]
        kind: Option<SourceKind>,

        /// Sync only a specific source instance
        #[arg(short = 'S', value_name = "ID")]
        source_id: Option<String>,
    },

    /// Inspect stored items
    List {
        /// Filter by source kind
        #[arg(short = 'k', value_name = "KIND")]
        kind: Option<SourceKind>,

        /// Filter by specific source ID
        #[arg(short = 'S', value_name = "ID")]
        source_id: Option<String>,

        /// Maximum number of items to display
        #[arg(short = 'n', value_name = "NUMBER", default_value = "20")]
        limit: usize,

        /// Only show items published at or after this time
        #[arg(short = 's', value_name = "TIME")]
        since: Option<String>,

        /// Filter items by substring in title/summary
        #[arg(short = 'q', value_name = "PATTERN")]
        query: Option<String>,
    },

    /// Produce feeds or export files
    Export {
        /// Filter by source kind
        #[arg(short = 'k', value_name = "KIND")]
        kind: Option<SourceKind>,

        /// Filter by specific source ID
        #[arg(short = 'S', value_name = "ID")]
        source_id: Option<String>,

        /// Maximum number of items
        #[arg(short = 'n', value_name = "NUMBER")]
        limit: Option<usize>,

        /// Only items published at or after this time
        #[arg(short = 's', value_name = "TIME")]
        since: Option<String>,

        /// Filter items by substring
        #[arg(short = 'q', value_name = "PATTERN")]
        query: Option<String>,

        /// Output format
        #[arg(short = 'f', value_name = "FORMAT", default_value = "json")]
        format: String,

        /// Output file (default: stdout)
        #[arg(short = 'o', value_name = "FILE")]
        output: Option<PathBuf>,
    },

    /// Self-host HTTP API
    Serve {
        /// Address to bind HTTP server to
        #[arg(short = 'a', value_name = "ADDRESS", default_value = "127.0.0.1:8080")]
        address: String,
    },

    /// Verify database schema and print statistics
    DbCheck,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Sync { all, kind, source_id } => handle_sync(cli.config_dir, cli.db_path, all, kind, source_id),
        Commands::List { kind, source_id, limit, since, query } => {
            handle_list(cli.db_path, kind, source_id, limit, since, query)
        }
        Commands::Export { kind, source_id, limit, since, query, format, output } => {
            handle_export(cli.db_path, kind, source_id, limit, since, query, format, output)
        }
        Commands::Serve { address } => handle_serve(cli.db_path, address),
        Commands::DbCheck => handle_db_check(cli.db_path),
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn handle_sync(
    config_dir: Option<PathBuf>, db_path: Option<PathBuf>, _all: bool, _kind: Option<SourceKind>,
    _source_id: Option<String>,
) -> Result<(), PaiError> {
    let db_path = paths::resolve_db_path(db_path)?;
    let _config_dir = paths::resolve_config_dir(config_dir)?;

    let storage = SqliteStorage::new(db_path)?;
    let config = Config::default();

    let count = pai_core::sync_all_sources(&config, &storage)?;

    println!("Synced {count} items");
    Ok(())
}

fn handle_list(
    db_path: Option<PathBuf>, kind: Option<SourceKind>, source_id: Option<String>, limit: usize, since: Option<String>,
    query: Option<String>,
) -> Result<(), PaiError> {
    let db_path = paths::resolve_db_path(db_path)?;
    let storage = SqliteStorage::new(db_path)?;

    let filter = ListFilter { source_kind: kind, source_id, limit: Some(limit), since, query };

    let items = pai_core::Storage::list_items(&storage, &filter)?;

    if items.is_empty() {
        println!("No items found");
        return Ok(());
    }

    println!("Found {} items:\n", items.len());
    for item in items {
        println!("ID: {}", item.id);
        println!("Source: {} ({})", item.source_kind, item.source_id);
        if let Some(title) = &item.title {
            println!("Title: {title}");
        }
        if let Some(author) = &item.author {
            println!("Author: {author}");
        }
        println!("URL: {}", item.url);
        println!("Published: {}", item.published_at);
        println!();
    }

    Ok(())
}

fn handle_export(
    db_path: Option<PathBuf>, kind: Option<SourceKind>, source_id: Option<String>, limit: Option<usize>,
    since: Option<String>, query: Option<String>, format: String, output: Option<PathBuf>,
) -> Result<(), PaiError> {
    let db_path = paths::resolve_db_path(db_path)?;
    let _storage = SqliteStorage::new(db_path)?;

    let filter = ListFilter { source_kind: kind, source_id, limit, since, query };

    println!("export command - format: {format}, output: {output:?}, filter: {filter:?}");
    Ok(())
}

fn handle_serve(db_path: Option<PathBuf>, address: String) -> Result<(), PaiError> {
    let db_path = paths::resolve_db_path(db_path)?;
    let _storage = SqliteStorage::new(db_path)?;

    println!("serve command - address: {address}");
    Ok(())
}

fn handle_db_check(db_path: Option<PathBuf>) -> Result<(), PaiError> {
    let db_path = paths::resolve_db_path(db_path)?;
    let storage = SqliteStorage::new(db_path)?;

    println!("Verifying database schema...");
    storage.verify_schema()?;
    println!("Schema verification: OK\n");

    println!("Database statistics:");
    let total = storage.count_items()?;
    println!("  Total items: {total}");

    let stats = storage.get_stats()?;
    if !stats.is_empty() {
        println!("\nItems by source:");
        for (source_kind, count) in stats {
            println!("  {source_kind}: {count}");
        }
    }

    Ok(())
}
