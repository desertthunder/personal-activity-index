mod paths;
mod storage;

use clap::{Parser, Subcommand};
use owo_colors::OwoColorize;
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

#[derive(Parser, Debug)]
struct ExportOpts {
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
}

impl From<ExportOpts> for ListFilter {
    fn from(opts: ExportOpts) -> Self {
        ListFilter {
            source_kind: opts.kind,
            source_id: opts.source_id,
            limit: opts.limit,
            since: opts.since,
            query: opts.query,
        }
    }
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
    Export(ExportOpts),

    /// Self-host HTTP API
    Serve {
        /// Address to bind HTTP server to
        #[arg(short = 'a', value_name = "ADDRESS", default_value = "127.0.0.1:8080")]
        address: String,
    },

    /// Verify database schema and print statistics
    DbCheck,

    /// Initialize configuration file
    Init {
        /// Force overwrite existing config
        #[arg(short = 'f')]
        force: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Sync { all, kind, source_id } => handle_sync(cli.config_dir, cli.db_path, all, kind, source_id),
        Commands::List { kind, source_id, limit, since, query } => {
            handle_list(cli.db_path, kind, source_id, limit, since, query)
        }
        Commands::Export(opts) => handle_export(cli.db_path, opts),
        Commands::Serve { address } => handle_serve(cli.db_path, address),
        Commands::DbCheck => handle_db_check(cli.db_path),
        Commands::Init { force } => handle_init(cli.config_dir, force),
    };

    if let Err(e) = result {
        eprintln!("{} {}", "Error:".red().bold(), e);
        std::process::exit(1);
    }
}

fn handle_sync(
    config_dir: Option<PathBuf>, db_path: Option<PathBuf>, _all: bool, kind: Option<SourceKind>,
    source_id: Option<String>,
) -> Result<(), PaiError> {
    let db_path = paths::resolve_db_path(db_path)?;
    let config_dir = paths::resolve_config_dir(config_dir)?;

    let storage = SqliteStorage::new(db_path)?;

    let config_path = config_dir.join("config.toml");
    let config = if config_path.exists() {
        Config::from_file(&config_path)?
    } else {
        println!(
            "{} No config file found, using default configuration",
            "Warning:".yellow()
        );
        Config::default()
    };

    let count = pai_core::sync_all_sources(&config, &storage, kind, source_id.as_deref())?;

    if count == 0 {
        println!("{} No sources synced (check your config or filters)", "Info:".cyan());
    } else {
        println!("{} Synced {}", "Success:".green(), format!("{count} source(s)").bold());
    }

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
        println!("{}", "No items found".yellow());
        return Ok(());
    }

    println!("{} {}\n", "Found".cyan(), format!("{} items:", items.len()).bold());
    for item in items {
        println!("{} {}", "ID:".bright_black(), item.id);
        println!(
            "{} {} {}",
            "Source:".bright_black(),
            item.source_kind.to_string().cyan(),
            format!("({})", item.source_id).bright_black()
        );
        if let Some(title) = &item.title {
            println!("{} {}", "Title:".bright_black(), title.bold());
        }
        if let Some(author) = &item.author {
            println!("{} {}", "Author:".bright_black(), author);
        }
        println!("{} {}", "URL:".bright_black(), item.url.blue().underline());
        println!("{} {}", "Published:".bright_black(), item.published_at);
        println!();
    }

    Ok(())
}

fn handle_export(db_path: Option<PathBuf>, opts: ExportOpts) -> Result<(), PaiError> {
    let db_path = paths::resolve_db_path(db_path)?;
    let _storage = SqliteStorage::new(db_path)?;

    let format = opts.format.clone();
    let output = opts.output.clone();
    let filter: ListFilter = opts.into();

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

    println!("{}", "Verifying database schema...".cyan());
    storage.verify_schema()?;
    println!("{} {}\n", "Schema verification:".green(), "OK".bold());

    println!("{}", "Database statistics:".cyan().bold());
    let total = storage.count_items()?;
    println!("  {}: {}", "Total items".bright_black(), total.to_string().bold());

    let stats = storage.get_stats()?;
    if !stats.is_empty() {
        println!("\n{}", "Items by source:".cyan().bold());
        for (source_kind, count) in stats {
            println!("  {}: {}", source_kind.bright_black(), count.to_string().bold());
        }
    }

    Ok(())
}

fn handle_init(config_dir: Option<PathBuf>, force: bool) -> Result<(), PaiError> {
    let config_dir = paths::resolve_config_dir(config_dir)?;
    let config_path = config_dir.join("config.toml");

    if config_path.exists() && !force {
        println!(
            "{} Config file already exists at {}",
            "Error:".red().bold(),
            config_path.display()
        );
        println!("{} Use {} to overwrite", "Hint:".yellow(), "pai init -f".bold());
        return Err(PaiError::Config("Config file already exists".to_string()));
    }

    std::fs::create_dir_all(&config_dir)
        .map_err(|e| PaiError::Config(format!("Failed to create config directory: {e}")))?;

    let default_config = include_str!("../../config.example.toml");
    std::fs::write(&config_path, default_config)
        .map_err(|e| PaiError::Config(format!("Failed to write config file: {e}")))?;

    println!("{} Created configuration file", "Success:".green().bold());
    println!(
        "  {}: {}",
        "Location".bright_black(),
        config_path.display().to_string().bold()
    );
    println!();
    println!("{}", "Next steps:".cyan().bold());
    println!("  1. Edit the config file to add your sources:");
    println!("     {}", format!("$EDITOR {}", config_path.display()).bright_black());
    println!("  2. Run sync to fetch content:");
    println!("     {}", "pai sync".bright_black());
    println!("  3. List your items:");
    println!("     {}", "pai list -n 10".bright_black());

    Ok(())
}
