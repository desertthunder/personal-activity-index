use clap::{Parser, Subcommand};
use pai_core::SourceKind;
use std::path::PathBuf;

/// Personal Activity Index - POSIX-style CLI for content aggregation
#[derive(Parser, Debug)]
#[command(name = "pai")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Set configuration directory
    #[arg(short = 'C', value_name = "DIR", global = true)]
    pub config_dir: Option<PathBuf>,

    /// Path to SQLite database file
    #[arg(short = 'd', value_name = "PATH", global = true)]
    pub db_path: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Parser, Debug)]
pub struct ExportOpts {
    /// Filter by source kind
    #[arg(short = 'k', value_name = "KIND")]
    pub kind: Option<SourceKind>,

    /// Filter by specific source ID
    #[arg(short = 'S', value_name = "ID")]
    pub source_id: Option<String>,

    /// Maximum number of items
    #[arg(short = 'n', value_name = "NUMBER")]
    pub limit: Option<usize>,

    /// Only items published at or after this time
    #[arg(short = 's', value_name = "TIME")]
    pub since: Option<String>,

    /// Filter items by substring
    #[arg(short = 'q', value_name = "PATTERN")]
    pub query: Option<String>,

    /// Output format
    #[arg(short = 'f', value_name = "FORMAT", default_value = "json")]
    pub format: String,

    /// Output file (default: stdout)
    #[arg(short = 'o', value_name = "FILE")]
    pub output: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
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

    /// Generate or install the pai(1) manpage
    Man {
        /// Output file (default: stdout)
        #[arg(short = 'o', value_name = "FILE")]
        output: Option<PathBuf>,

        /// Install into a manpath directory (defaults to ~/.local/share/man if unset)
        #[arg(long)]
        install: bool,

        /// Custom directory for --install (e.g., /usr/local/share/man)
        #[arg(long, value_name = "DIR")]
        install_dir: Option<PathBuf>,
    },

    /// Initialize Cloudflare Worker deployment scaffolding
    #[command(name = "cf-init")]
    CfInit {
        /// Output directory for scaffolding (default: current directory)
        #[arg(short = 'o', value_name = "DIR")]
        output_dir: Option<PathBuf>,

        /// Dry run - show what would be created without writing files
        #[arg(long)]
        dry_run: bool,
    },
}
