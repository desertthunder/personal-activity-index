mod app;
mod paths;

use app::{Cli, Commands, ExportOpts};
use chrono::{DateTime, Duration, Utc};
use clap::Parser;
use owo_colors::OwoColorize;
use pai_core::{Config, Item, ListFilter, PaiError, SourceKind};
use pai_server::SqliteStorage;
use rss::{Channel, ChannelBuilder, ItemBuilder};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

const PUBLISHED_WIDTH: usize = 19;
const KIND_WIDTH: usize = 9;
const SOURCE_WIDTH: usize = 24;
const TITLE_WIDTH: usize = 60;
const MAN_PAGE: &str = include_str!(env!("PAI_MAN_PAGE"));

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
        Commands::Man { output, install, install_dir } => handle_man(output, install, install_dir),
        Commands::CfInit { output_dir, dry_run } => handle_cf_init(output_dir, dry_run),
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

    let since = normalize_since_input(since)?;
    let limit = ensure_positive_limit(limit)?;
    let source_id = normalize_optional_string(source_id);
    let query = normalize_optional_string(query);

    let filter = ListFilter { source_kind: kind, source_id, limit: Some(limit), since, query };

    let items = pai_core::Storage::list_items(&storage, &filter)?;

    if items.is_empty() {
        println!("{}", "No items found".yellow());
        return Ok(());
    }

    println!("{} {}", "Found".cyan(), format!("{} item(s)", items.len()).bold());
    println!();
    render_items_table(&items)?;

    Ok(())
}

fn handle_export(db_path: Option<PathBuf>, opts: ExportOpts) -> Result<(), PaiError> {
    let db_path = paths::resolve_db_path(db_path)?;
    let storage = SqliteStorage::new(db_path)?;

    let ExportOpts { kind, source_id, limit, since, query, format, output } = opts;
    let since = normalize_since_input(since)?;
    let limit = ensure_optional_limit(limit)?;
    let source_id = normalize_optional_string(source_id);
    let query = normalize_optional_string(query);

    let filter = ListFilter { source_kind: kind, source_id, limit, since, query };
    let items = pai_core::Storage::list_items(&storage, &filter)?;

    let export_format = ExportFormat::from_str(&format)?;
    let mut writer = create_output_writer(output.as_ref())?;
    export_items(&items, export_format, writer.as_mut())?;

    match output {
        Some(path) => println!(
            "{} Exported {} item(s) to {}",
            "Success:".green(),
            items.len(),
            path.display()
        ),
        None => println!("{} Exported {} item(s) to stdout", "Success:".green(), items.len()),
    }

    Ok(())
}

fn handle_serve(db_path: Option<PathBuf>, address: String) -> Result<(), PaiError> {
    let db_path = paths::resolve_db_path(db_path)?;
    pai_server::serve(db_path, &address)
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

    fs::create_dir_all(&config_dir).map_err(|e| PaiError::Config(format!("Failed to create config directory: {e}")))?;

    let default_config = include_str!("../../config.example.toml");
    fs::write(&config_path, default_config)
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

fn handle_man(output: Option<PathBuf>, install: bool, install_dir: Option<PathBuf>) -> Result<(), PaiError> {
    if install && output.is_some() {
        return Err(PaiError::InvalidArgument(
            "Use either --install or -o/--output when generating manpages".to_string(),
        ));
    }

    let target = if install { Some(resolve_man_install_path(install_dir)?) } else { output };

    let mut writer = create_output_writer(target.as_ref())?;
    writer.write_all(MAN_PAGE.as_bytes()).map_err(PaiError::Io)?;
    writer.flush().map_err(PaiError::Io)?;

    if let Some(path) = target {
        if install {
            println!("{} Installed manpage to {}", "Success:".green(), path.display());
            if let Some(root) = man_root_for(&path) {
                println!(
                    "{} Ensure {} is on your MANPATH, then run {}",
                    "Hint:".yellow(),
                    root.display(),
                    "man pai".bright_black()
                );
            } else {
                println!(
                    "{} Run man pai after adding the install dir to MANPATH.",
                    "Hint:".yellow()
                );
            }
        } else {
            println!("{} Wrote manpage to {}", "Success:".green(), path.display());
        }
    }

    Ok(())
}

fn resolve_man_install_path(custom_dir: Option<PathBuf>) -> Result<PathBuf, PaiError> {
    let base = if let Some(dir) = custom_dir { dir } else { find_writable_man_dir()? };

    let install_dir = if base.file_name().map(|os| os == "man1").unwrap_or(false) { base } else { base.join("man1") };

    fs::create_dir_all(&install_dir).map_err(|e| {
        PaiError::Io(io::Error::new(
            e.kind(),
            format!("Failed to create man directory {}: {}", install_dir.display(), e),
        ))
    })?;

    Ok(install_dir.join("pai.1"))
}

fn find_writable_man_dir() -> Result<PathBuf, PaiError> {
    let candidates = [
        dirs::data_local_dir().map(|d| d.join("man")),
        dirs::home_dir().map(|d| d.join(".local/share/man")),
        Some(PathBuf::from("/usr/local/share/man")),
        Some(PathBuf::from("/opt/homebrew/share/man")),
        Some(PathBuf::from("/usr/local/Homebrew/share/man")),
    ];

    for candidate in candidates.iter().flatten() {
        if candidate.exists() {
            let test_file = candidate.join(".pai-write-test");
            if fs::write(&test_file, b"test").is_ok() {
                let _ = fs::remove_file(&test_file);
                return Ok(candidate.clone());
            }
        } else if let Some(parent) = candidate.parent() {
            if parent.exists() {
                let test_dir = candidate.join("man1");
                if fs::create_dir_all(&test_dir).is_ok() {
                    let _ = fs::remove_dir_all(&test_dir);
                    return Ok(candidate.clone());
                }
            }
        }
    }

    if let Some(data_dir) = dirs::data_local_dir() {
        return Ok(data_dir.join("man"));
    }

    Err(PaiError::Config(
        "Unable to find a writable man page directory. Use --install-dir to specify one.".to_string(),
    ))
}

fn man_root_for(path: &Path) -> Option<&Path> {
    path.parent()?.parent()
}

fn handle_cf_init(output_dir: Option<PathBuf>, dry_run: bool) -> Result<(), PaiError> {
    let target_dir = output_dir.unwrap_or_else(|| PathBuf::from("."));

    let wrangler_template = include_str!("../../worker/wrangler.example.toml");
    let schema_sql = include_str!("../../worker/schema.sql");

    let readme_content = r#"# Cloudflare Worker Deployment

## Quick Start

1. **Create D1 Database:**
   ```sh
   wrangler d1 create personal-activity-db
   ```

2. **Copy the configuration:**
   ```sh
   cp wrangler.example.toml wrangler.toml
   ```

3. **Update `wrangler.toml`:**
   - Replace `{DATABASE_ID}` with the ID from step 1
   - Adjust `name` and `database_name` if desired

4. **Initialize the database schema:**
   ```sh
   wrangler d1 execute personal-activity-db --file=schema.sql
   ```

5. **Build the worker:**
   ```sh
   cd ..
   cargo install worker-build
   worker-build --release -p pai-worker
   ```

6. **Deploy:**
   ```sh
   cd worker
   wrangler deploy
   ```

## Testing Locally

Run the worker locally with:
```sh
wrangler dev
```

## Scheduled Syncs

The worker is configured with a cron trigger (see `wrangler.toml`). The default schedule runs every hour.
To modify the schedule, edit the `crons` array in `wrangler.toml`.

## API Endpoints

- `GET /api/feed` - List items with optional filters
- `GET /api/item/:id` - Get a single item by ID
- `GET /status` - Health check

## Environment Variables

Configure in `wrangler.toml` under `[vars]`:
- `LOG_LEVEL` - Set logging verbosity (optional)
"#;

    let files = vec![
        ("wrangler.example.toml", wrangler_template),
        ("schema.sql", schema_sql),
        ("README.md", readme_content),
    ];

    if dry_run {
        println!("{} Dry run - showing files that would be created:\n", "Info:".cyan());
        for (filename, content) in &files {
            let path = target_dir.join(filename);
            println!("  {} {}", "Would create:".bright_black(), path.display());
            println!("    {} bytes", content.len());
        }
        println!("\n{} Run without --dry-run to create these files", "Hint:".yellow());
        return Ok(());
    }

    fs::create_dir_all(&target_dir)?;

    for (filename, content) in &files {
        let path = target_dir.join(filename);
        if path.exists() {
            println!("{} {} already exists, skipping", "Warning:".yellow(), filename);
            continue;
        }
        fs::write(&path, content)?;
        println!("{} Created {}", "Success:".green(), path.display());
    }

    println!("\n{} Cloudflare Worker scaffolding created!", "Success:".green().bold());
    println!("\n{} Next steps:", "Info:".cyan());
    println!("  1. cd {}", target_dir.display());
    println!("  2. Read README.md for deployment instructions");
    println!("  3. wrangler d1 create personal-activity-db");
    println!("  4. Update wrangler.example.toml with your database ID");

    Ok(())
}

fn normalize_since_input(since: Option<String>) -> Result<Option<String>, PaiError> {
    normalize_since_with_now(since, Utc::now())
}

fn normalize_since_with_now(since: Option<String>, now: DateTime<Utc>) -> Result<Option<String>, PaiError> {
    let value = match since {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed.to_string()
        }
        None => return Ok(None),
    };

    if let Some(duration) = parse_relative_duration(&value) {
        let instant = now - duration;
        return Ok(Some(instant.to_rfc3339()));
    }

    if let Ok(dt) = DateTime::parse_from_rfc3339(&value) {
        return Ok(Some(dt.with_timezone(&Utc).to_rfc3339()));
    }

    if let Ok(dt) = DateTime::parse_from_rfc2822(&value) {
        return Ok(Some(dt.with_timezone(&Utc).to_rfc3339()));
    }

    let msg = format!(
        "Invalid since value '{value}'. Use ISO 8601 (e.g. 2024-01-01T00:00:00Z) or relative forms like 7d/24h/60m."
    );
    Err(PaiError::InvalidArgument(msg))
}

fn parse_relative_duration(input: &str) -> Option<Duration> {
    if input.len() < 2 {
        return None;
    }

    let unit = input.chars().last()?.to_ascii_lowercase();
    let magnitude: i64 = input[..input.len() - 1].parse().ok()?;

    match unit {
        'm' => Some(Duration::minutes(magnitude)),
        'h' => Some(Duration::hours(magnitude)),
        'd' => Some(Duration::days(magnitude)),
        'w' => Some(Duration::weeks(magnitude)),
        _ => None,
    }
}

fn ensure_positive_limit(limit: usize) -> Result<usize, PaiError> {
    if limit == 0 {
        return Err(PaiError::InvalidArgument("Limit must be greater than zero".to_string()));
    }
    Ok(limit)
}

fn ensure_optional_limit(limit: Option<usize>) -> Result<Option<usize>, PaiError> {
    match limit {
        Some(value) => Ok(Some(ensure_positive_limit(value)?)),
        None => Ok(None),
    }
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|input| {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

enum ExportFormat {
    Json,
    Ndjson,
    Rss,
}

impl FromStr for ExportFormat {
    type Err = PaiError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "ndjson" => Ok(Self::Ndjson),
            "rss" => Ok(Self::Rss),
            other => Err(PaiError::InvalidArgument(format!(
                "Unsupported export format '{other}'. Expected json, ndjson, or rss."
            ))),
        }
    }
}

fn create_output_writer(path: Option<&PathBuf>) -> Result<Box<dyn Write>, PaiError> {
    if let Some(path) = path {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let file = fs::File::create(path)?;
        Ok(Box::new(file))
    } else {
        Ok(Box::new(io::stdout()))
    }
}

fn export_items(items: &[Item], format: ExportFormat, writer: &mut dyn Write) -> Result<(), PaiError> {
    match format {
        ExportFormat::Json => write_json(items, writer)?,
        ExportFormat::Ndjson => write_ndjson(items, writer)?,
        ExportFormat::Rss => write_rss(items, writer)?,
    }

    writer.flush().map_err(PaiError::Io)
}

fn write_json(items: &[Item], writer: &mut dyn Write) -> Result<(), PaiError> {
    serde_json::to_writer_pretty(&mut *writer, items)
        .map_err(|e| PaiError::Parse(format!("Failed to serialize JSON export: {e}")))?;
    writer.write_all(b"\n").map_err(PaiError::Io)
}

fn write_ndjson(items: &[Item], writer: &mut dyn Write) -> Result<(), PaiError> {
    for item in items {
        serde_json::to_writer(&mut *writer, item)
            .map_err(|e| PaiError::Parse(format!("Failed to serialize JSON export: {e}")))?;
        writer.write_all(b"\n").map_err(PaiError::Io)?;
    }
    Ok(())
}

fn write_rss(items: &[Item], writer: &mut dyn Write) -> Result<(), PaiError> {
    let channel = build_rss_channel(items)?;
    let rss_string = channel.to_string();
    writer.write_all(rss_string.as_bytes()).map_err(PaiError::Io)?;
    writer.write_all(b"\n").map_err(PaiError::Io)
}

fn build_rss_channel(items: &[Item]) -> Result<Channel, PaiError> {
    const TITLE: &str = "Personal Activity Index";
    const LINK: &str = "https://personal-activity-index.local/";
    const DESCRIPTION: &str = "Aggregated feed exported by the Personal Activity Index CLI.";

    let rss_items: Vec<rss::Item> = items
        .iter()
        .map(|item| {
            let title = item
                .title
                .as_deref()
                .or(item.summary.as_deref())
                .unwrap_or(&item.url)
                .to_string();
            let description = item
                .summary
                .as_deref()
                .or(item.content_html.as_deref())
                .unwrap_or("")
                .to_string();
            let author = item.author.as_deref().unwrap_or("Unknown").to_string();
            let pub_date = format_rss_date(&item.published_at);

            ItemBuilder::default()
                .title(Some(title))
                .link(Some(item.url.clone()))
                .guid(Some(
                    rss::GuidBuilder::default().value(&item.id).permalink(false).build(),
                ))
                .pub_date(Some(pub_date))
                .author(Some(author))
                .description(Some(description))
                .categories(vec![rss::CategoryBuilder::default()
                    .name(item.source_kind.to_string())
                    .build()])
                .build()
        })
        .collect();

    let channel = ChannelBuilder::default()
        .title(TITLE)
        .link(LINK)
        .description(DESCRIPTION)
        .items(rss_items)
        .build();

    Ok(channel)
}

fn format_rss_date(value: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        dt.to_rfc2822()
    } else if let Ok(dt) = DateTime::parse_from_rfc2822(value) {
        dt.to_rfc2822()
    } else {
        value.to_string()
    }
}

fn format_published_display(value: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        dt.with_timezone(&Utc).format("%Y-%m-%d %H:%M").to_string()
    } else if let Ok(dt) = DateTime::parse_from_rfc2822(value) {
        dt.with_timezone(&Utc).format("%Y-%m-%d %H:%M").to_string()
    } else {
        value.to_string()
    }
}

fn truncate_for_column(value: &str, max_chars: usize) -> String {
    let total_chars = value.chars().count();
    if total_chars <= max_chars {
        return value.to_string();
    }

    if max_chars <= 3 {
        return value.chars().take(max_chars).collect();
    }

    let mut truncated = String::new();
    for ch in value.chars().take(max_chars - 3) {
        truncated.push(ch);
    }
    truncated.push_str("...");
    truncated
}

fn render_items_table(items: &[Item]) -> Result<(), PaiError> {
    let mut stdout = io::stdout();
    write_items_table(items, &mut stdout).map_err(PaiError::Io)
}

fn write_items_table<W: Write>(items: &[Item], writer: &mut W) -> io::Result<()> {
    let header = format!(
        "| {published:<pub_width$} | {kind:<kind_width$} | {source:<source_width$} | {title:<title_width$} |",
        published = "Published",
        kind = "Kind",
        source = "Source",
        title = "Title",
        pub_width = PUBLISHED_WIDTH,
        kind_width = KIND_WIDTH,
        source_width = SOURCE_WIDTH,
        title_width = TITLE_WIDTH,
    );
    let separator = "-".repeat(header.len());

    writeln!(writer, "{separator}")?;
    writeln!(writer, "{header}")?;
    writeln!(writer, "{}", separator.clone())?;

    for item in items {
        let published = truncate_for_column(&format_published_display(&item.published_at), PUBLISHED_WIDTH);
        let kind = truncate_for_column(&item.source_kind.to_string(), KIND_WIDTH);
        let source = truncate_for_column(&item.source_id, SOURCE_WIDTH);
        let title_text = item.title.as_deref().or(item.summary.as_deref()).unwrap_or(&item.url);
        let title = truncate_for_column(title_text, TITLE_WIDTH);

        let row = format!(
            "| {published:<PUBLISHED_WIDTH$} | {kind:<KIND_WIDTH$} | {source:<SOURCE_WIDTH$} | {title:<TITLE_WIDTH$} |",
        );
        writeln!(writer, "{row}")?;
    }

    writeln!(writer, "{separator}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_item() -> Item {
        Item {
            id: "sample-id".to_string(),
            source_kind: SourceKind::Substack,
            source_id: "patternmatched.substack.com".to_string(),
            author: Some("Pattern Matched".to_string()),
            title: Some("Test entry".to_string()),
            summary: Some("Summary".to_string()),
            url: "https://patternmatched.substack.com/p/test".to_string(),
            content_html: None,
            published_at: "2024-01-01T00:00:00Z".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn normalize_since_accepts_iso8601() {
        let now = Utc.with_ymd_and_hms(2024, 1, 10, 0, 0, 0).unwrap();
        let since = normalize_since_with_now(Some("2024-01-01T00:00:00Z".to_string()), now).unwrap();
        assert_eq!(since.unwrap(), "2024-01-01T00:00:00+00:00");
    }

    #[test]
    fn normalize_since_accepts_relative_days() {
        let now = Utc.with_ymd_and_hms(2024, 1, 10, 0, 0, 0).unwrap();
        let since = normalize_since_with_now(Some("3d".to_string()), now).unwrap();
        assert_eq!(since.unwrap(), "2024-01-07T00:00:00+00:00");
    }

    #[test]
    fn ensure_positive_limit_rejects_zero() {
        assert!(ensure_positive_limit(0).is_err());
        assert!(ensure_optional_limit(Some(0)).is_err());
    }

    #[test]
    fn export_format_parsing() {
        assert!(matches!(ExportFormat::from_str("json").unwrap(), ExportFormat::Json));
        assert!(matches!(
            ExportFormat::from_str("NDJSON").unwrap(),
            ExportFormat::Ndjson
        ));
        assert!(matches!(ExportFormat::from_str("rss").unwrap(), ExportFormat::Rss));
        assert!(ExportFormat::from_str("invalid").is_err());
    }

    #[test]
    fn json_export_serializes_items() {
        let mut buffer = Vec::new();
        export_items(&[sample_item()], ExportFormat::Json, &mut buffer).unwrap();
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.trim_start().starts_with('['));
        assert!(output.contains("sample-id"));
    }

    #[test]
    fn ndjson_export_serializes_items() {
        let mut buffer = Vec::new();
        export_items(&[sample_item()], ExportFormat::Ndjson, &mut buffer).unwrap();
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.lines().next().unwrap().contains("sample-id"));
    }

    #[test]
    fn rss_export_contains_items() {
        let channel = build_rss_channel(&[sample_item()]).unwrap();
        let feed = channel.to_string();
        assert!(feed.contains("<rss"));
        assert!(feed.contains("<item>"));
        assert!(feed.contains("sample-id"));
    }

    #[test]
    fn table_writer_emits_rows() {
        let mut buffer = Vec::new();
        write_items_table(&[sample_item()], &mut buffer).unwrap();
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Published"));
        assert!(output.contains("patternmatched"));
    }

    #[test]
    fn truncate_column_adds_ellipsis() {
        let truncated = truncate_for_column("abcdefghijklmnopqrstuvwxyz", 8);
        assert_eq!(truncated, "abcde...");
    }

    #[test]
    fn manpage_contains_name_section() {
        assert!(MAN_PAGE.contains("NAME"));
        assert!(MAN_PAGE.contains("pai"));
    }
}
