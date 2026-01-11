//! xf - Ultra-fast X data archive search CLI
//!
//! Main entry point for the xf command-line tool.

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::ThreadPoolBuilder;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use tracing::{Level, info};
use tracing_subscriber::EnvFilter;

use xf::cli;
use xf::config::Config;
use xf::search;
use xf::stats_analytics::{self, ContentStats, EngagementStats, TemporalStats};
use xf::{
    ArchiveParser, ArchiveStats, Cli, Commands, DataType, ExportFormat, ExportTarget, ListTarget,
    OutputFormat, SearchEngine, SearchResult, SearchResultType, SortOrder, Storage, TweetUrl,
};

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    let log_level = if cli.verbose {
        Level::DEBUG
    } else if cli.quiet {
        Level::ERROR
    } else {
        Level::INFO
    };

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(log_level.into()))
        .with_target(false)
        .without_time()
        .init();

    // Run the appropriate command
    match &cli.command {
        Commands::Index(args) => cmd_index(&cli, args),
        Commands::Search(args) => cmd_search(&cli, args),
        Commands::Stats(args) => cmd_stats(&cli, args),
        Commands::Tweet(args) => cmd_tweet(&cli, args),
        Commands::List(args) => cmd_list(&cli, args),
        Commands::Export(args) => cmd_export(&cli, args),
        Commands::Config(args) => cmd_config(&cli, args),
        Commands::Update => {
            cmd_update();
            Ok(())
        }
        Commands::Completions(args) => {
            cmd_completions(args);
            Ok(())
        }
    }
}

fn get_db_path(cli: &Cli) -> PathBuf {
    if let Some(db) = &cli.db {
        return db.clone();
    }
    let config = Config::load();
    config.db_path()
}

fn get_index_path(cli: &Cli) -> PathBuf {
    if let Some(index) = &cli.index {
        return index.clone();
    }
    let config = Config::load();
    config.index_path()
}

#[allow(clippy::too_many_lines)]
fn cmd_index(cli: &Cli, args: &cli::IndexArgs) -> Result<()> {
    let archive_path = &args.archive_path;

    // Validate archive path
    if !archive_path.exists() {
        anyhow::bail!("Archive path does not exist: {}", archive_path.display());
    }

    let data_path = archive_path.join("data");
    if !data_path.exists() {
        anyhow::bail!(
            "Invalid archive: no 'data' directory found at {}",
            archive_path.display()
        );
    }

    if args.jobs > 0 {
        ThreadPoolBuilder::new()
            .num_threads(args.jobs)
            .build_global()
            .context("Failed to configure rayon thread pool")?;
    }

    // Setup database and index paths
    let db_path = get_db_path(cli);
    let index_path = get_index_path(cli);

    // Create data directory
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&index_path)?;

    // Handle force flag
    if args.force {
        if db_path.exists() {
            std::fs::remove_file(&db_path)?;
        }
        if index_path.exists() {
            std::fs::remove_dir_all(&index_path)?;
            std::fs::create_dir_all(&index_path)?;
        }
        info!("Cleared existing data");
    }

    println!("{}", "Indexing X data archive...".bold().cyan());
    println!("  Archive: {}", archive_path.display());
    println!("  Database: {}", db_path.display());
    println!("  Index: {}", index_path.display());
    println!();

    // Parse archive
    let parser = ArchiveParser::new(archive_path);

    // Open storage and search engine
    let mut storage = Storage::open(&db_path)?;
    let search_engine = SearchEngine::open(&index_path)?;
    let mut writer = search_engine.writer(100_000_000)?;

    // Parse and store manifest
    let manifest = parser.parse_manifest()?;
    storage.store_archive_info(&manifest)?;
    println!(
        "  {} Archive for @{} ({})",
        "✓".green(),
        manifest.username,
        manifest.display_name.as_deref().unwrap_or("Unknown")
    );

    // Determine what to index
    let data_types = args.only.as_ref().map_or_else(
        || {
            args.skip.as_ref().map_or_else(DataType::all, |skip| {
                DataType::all()
                    .into_iter()
                    .filter(|t| !skip.contains(t))
                    .collect()
            })
        },
        Clone::clone,
    );

    // Progress bar
    let pb = ProgressBar::new(data_types.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("##-"),
    );

    // Index each data type
    for data_type in &data_types {
        match data_type {
            DataType::Tweet => {
                pb.set_message("Indexing tweets...");
                let tweets = parser.parse_tweets()?;
                storage.store_tweets(&tweets)?;
                search_engine.index_tweets(&mut writer, &tweets)?;
                pb.println(format!("  {} {} tweets", "✓".green(), tweets.len()));
            }
            DataType::Like => {
                pb.set_message("Indexing likes...");
                let likes = parser.parse_likes()?;
                storage.store_likes(&likes)?;
                search_engine.index_likes(&mut writer, &likes)?;
                pb.println(format!("  {} {} likes", "✓".green(), likes.len()));
            }
            DataType::Dm => {
                pb.set_message("Indexing DMs...");
                let convos = parser.parse_direct_messages()?;
                let msg_count: usize = convos.iter().map(|c| c.messages.len()).sum();
                storage.store_dm_conversations(&convos)?;
                search_engine.index_dms(&mut writer, &convos)?;
                pb.println(format!(
                    "  {} {} DM conversations ({} messages)",
                    "✓".green(),
                    convos.len(),
                    msg_count
                ));
            }
            DataType::Grok => {
                pb.set_message("Indexing Grok messages...");
                let messages = parser.parse_grok_messages()?;
                storage.store_grok_messages(&messages)?;
                search_engine.index_grok_messages(&mut writer, &messages)?;
                pb.println(format!(
                    "  {} {} Grok messages",
                    "✓".green(),
                    messages.len()
                ));
            }
            DataType::Follower => {
                pb.set_message("Indexing followers...");
                let followers = parser.parse_followers()?;
                storage.store_followers(&followers)?;
                pb.println(format!("  {} {} followers", "✓".green(), followers.len()));
            }
            DataType::Following => {
                pb.set_message("Indexing following...");
                let following = parser.parse_following()?;
                storage.store_following(&following)?;
                pb.println(format!("  {} {} following", "✓".green(), following.len()));
            }
            DataType::Block => {
                pb.set_message("Indexing blocks...");
                let blocks = parser.parse_blocks()?;
                storage.store_blocks(&blocks)?;
                pb.println(format!("  {} {} blocks", "✓".green(), blocks.len()));
            }
            DataType::Mute => {
                pb.set_message("Indexing mutes...");
                let mutes = parser.parse_mutes()?;
                storage.store_mutes(&mutes)?;
                pb.println(format!("  {} {} mutes", "✓".green(), mutes.len()));
            }
            DataType::All => {
                // Already handled by DataType::all()
            }
        }
        pb.inc(1);
    }

    pb.finish_and_clear();

    // Commit search index
    writer.commit()?;
    search_engine.reload()?;

    println!();
    println!("{}", "Indexing complete!".bold().green());
    println!(
        "  Total documents indexed: {}",
        search_engine.doc_count().to_string().cyan()
    );
    println!();
    println!("Run {} to search your archive.", "xf search <query>".bold());

    Ok(())
}

#[allow(clippy::too_many_lines)]
fn cmd_search(cli: &Cli, args: &cli::SearchArgs) -> Result<()> {
    let db_path = get_db_path(cli);
    let index_path = get_index_path(cli);

    if !db_path.exists() {
        anyhow::bail!(
            "No indexed archive found. Run 'xf index <archive_path>' first.\n\
             Expected database at: {}",
            db_path.display()
        );
    }

    if !index_path.join("meta.json").exists() {
        anyhow::bail!(
            "No search index found. Run 'xf index <archive_path>' first.\n\
             Expected index at: {}",
            index_path.display()
        );
    }

    if args.replies_only && args.no_replies {
        anyhow::bail!("Cannot use --replies-only and --no-replies together.");
    }

    if args.context {
        if !matches!(
            cli.format,
            OutputFormat::Text | OutputFormat::Json | OutputFormat::JsonPretty
        ) {
            anyhow::bail!("--context only supports text or json output.");
        }
        if let Some(types) = &args.types {
            if types.len() != 1 || !types.contains(&DataType::Dm) {
                anyhow::bail!("--context only supports --types dm.");
            }
        }
    }

    if let Some(fields) = &args.fields {
        if args.context {
            anyhow::bail!("--fields is not supported with --context.");
        }
        if !matches!(cli.format, OutputFormat::Json | OutputFormat::JsonPretty) {
            anyhow::bail!("--fields is only supported with --format json or json-pretty.");
        }
        validate_output_fields(fields)?;
    }

    let search_engine = SearchEngine::open(&index_path)?;
    let storage = if args.context {
        Some(Storage::open(&db_path)?)
    } else {
        None
    };

    // Convert data types to search doc types
    let doc_types: Option<Vec<search::DocType>> = if args.context {
        Some(vec![search::DocType::DirectMessage])
    } else {
        args.types.as_ref().map(|types| {
            types
                .iter()
                .filter_map(|t| match t {
                    DataType::Tweet => Some(search::DocType::Tweet),
                    DataType::Like => Some(search::DocType::Like),
                    DataType::Dm => Some(search::DocType::DirectMessage),
                    DataType::Grok => Some(search::DocType::GrokMessage),
                    _ => None,
                })
                .collect()
        })
    };

    let mut results = search_engine.search(
        &args.query,
        doc_types.as_deref(),
        args.limit.saturating_add(args.offset),
    )?;

    apply_search_filters(&mut results, args)?;
    apply_search_sort(&mut results, &args.sort);

    // Apply offset
    let mut results: Vec<_> = results.into_iter().skip(args.offset).collect();
    if args.limit == 0 {
        results.clear();
    } else if results.len() > args.limit {
        results.truncate(args.limit);
    }

    if results.is_empty() {
        println!("{}", "No results found.".yellow());
        return Ok(());
    }

    if args.context {
        let contexts = build_dm_context(&results, storage.as_ref().unwrap())?;
        output_dm_context(cli, &contexts)?;
        return Ok(());
    }

    // Output results
    match cli.format {
        OutputFormat::Json => {
            if let Some(fields) = &args.fields {
                let filtered = filter_results_fields(&results, fields)?;
                println!("{}", serde_json::to_string(&filtered)?);
            } else {
                println!("{}", serde_json::to_string(&results)?);
            }
        }
        OutputFormat::JsonPretty => {
            if let Some(fields) = &args.fields {
                let filtered = filter_results_fields(&results, fields)?;
                println!("{}", serde_json::to_string_pretty(&filtered)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&results)?);
            }
        }
        OutputFormat::Csv => {
            println!("type,id,created_at,score,text");
            for r in &results {
                println!(
                    "{},{},{},{:.4},\"{}\"",
                    r.result_type,
                    r.id,
                    r.created_at.to_rfc3339(),
                    r.score,
                    r.text.replace('"', "\"\"")
                );
            }
        }
        OutputFormat::Compact => {
            for r in &results {
                println!("[{}] {} | {}", r.result_type, r.id, truncate(&r.text, 100));
            }
        }
        OutputFormat::Text => {
            println!(
                "{} results for \"{}\":\n",
                results.len().to_string().cyan(),
                args.query.bold()
            );

            for (i, r) in results.iter().enumerate() {
                print_result(i + 1, r);
            }
        }
    }

    Ok(())
}

#[derive(Serialize)]
struct DmConversationContext {
    conversation_id: String,
    messages: Vec<DmContextMessage>,
}

#[derive(Serialize)]
struct DmContextMessage {
    id: String,
    sender_id: String,
    recipient_id: String,
    text: String,
    created_at: DateTime<Utc>,
    urls: Vec<TweetUrl>,
    media_urls: Vec<String>,
    is_match: bool,
    highlights: Vec<String>,
}

fn build_dm_context(
    results: &[SearchResult],
    storage: &Storage,
) -> Result<Vec<DmConversationContext>> {
    let mut conversation_order = Vec::new();
    let mut seen = HashSet::new();
    let mut matches: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();

    for result in results {
        let conv_id = result
            .metadata
            .get("conversation_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow::anyhow!("DM result missing conversation_id metadata"))?;
        let conv_id = conv_id.to_string();

        if seen.insert(conv_id.clone()) {
            conversation_order.push(conv_id.clone());
        }

        matches
            .entry(conv_id)
            .or_default()
            .entry(result.id.clone())
            .or_insert_with(|| result.highlights.clone());
    }

    let mut contexts = Vec::with_capacity(conversation_order.len());
    for conversation_id in conversation_order {
        let messages = storage.get_conversation_messages(&conversation_id)?;
        let mut context_messages = Vec::with_capacity(messages.len());

        let message_matches = matches.get(&conversation_id);
        for message in messages {
            let (is_match, highlights) = match message_matches {
                Some(match_map) => match_map
                    .get(&message.id)
                    .map_or((false, Vec::new()), |items| (true, items.clone())),
                None => (false, Vec::new()),
            };

            context_messages.push(DmContextMessage {
                id: message.id,
                sender_id: message.sender_id,
                recipient_id: message.recipient_id,
                text: message.text,
                created_at: message.created_at,
                urls: message.urls,
                media_urls: message.media_urls,
                is_match,
                highlights,
            });
        }

        contexts.push(DmConversationContext {
            conversation_id,
            messages: context_messages,
        });
    }

    Ok(contexts)
}

fn output_dm_context(cli: &Cli, contexts: &[DmConversationContext]) -> Result<()> {
    match cli.format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(contexts)?);
        }
        OutputFormat::JsonPretty => {
            println!("{}", serde_json::to_string_pretty(contexts)?);
        }
        OutputFormat::Text => {
            print_dm_context_text(contexts);
        }
        _ => {
            anyhow::bail!("--context only supports text or json output.");
        }
    }
    Ok(())
}

fn print_dm_context_text(contexts: &[DmConversationContext]) {
    for context in contexts {
        println!(
            "{} {}",
            "Conversation".bold().cyan(),
            context.conversation_id.dimmed()
        );
        println!("{}", "─".repeat(60));

        for message in &context.messages {
            let timestamp = message.created_at.format("%Y-%m-%d %H:%M").to_string();
            println!(
                "{} {} {} {}",
                timestamp.dimmed(),
                message.sender_id.green(),
                "→".dimmed(),
                message.recipient_id.blue()
            );

            let lines = textwrap::wrap(&message.text, 78);
            for line in lines {
                if message.is_match {
                    println!("  {}", line.yellow().bold());
                } else {
                    println!("  {line}");
                }
            }
            println!();
        }
    }
}

fn print_result(num: usize, result: &SearchResult) {
    let type_badge = match result.result_type {
        SearchResultType::Tweet => "TWEET".on_blue(),
        SearchResultType::Like => "LIKE".on_magenta(),
        SearchResultType::DirectMessage => "DM".on_green(),
        SearchResultType::GrokMessage => "GROK".on_yellow(),
    };

    println!(
        "{}. {} {} {}",
        num.to_string().dimmed(),
        type_badge,
        result.id.dimmed(),
        format!("({:.2})", result.score).dimmed()
    );

    // Use highlighted text if available, otherwise use plain text
    let display_text = if result.highlights.is_empty() {
        result.text.clone()
    } else {
        // Convert HTML highlights to ANSI colors
        // Tantivy uses <b> tags for highlighting
        html_highlights_to_ansi(&result.highlights[0])
    };

    // Word wrap the text
    let wrapped = textwrap::wrap(&display_text, 78);
    for line in wrapped {
        println!("   {line}");
    }

    if result.created_at.timestamp() > 0 {
        println!(
            "   {}",
            result
                .created_at
                .format("%Y-%m-%d %H:%M")
                .to_string()
                .dimmed()
        );
    }

    println!();
}

/// Convert HTML-style highlights (from Tantivy) to ANSI colored text
fn html_highlights_to_ansi(html: &str) -> String {
    // Tantivy uses <b>...</b> for highlighting
    // We'll convert these to ANSI bold + yellow
    let mut result = html.to_string();

    // Replace opening tags with ANSI escape for bold yellow
    result = result.replace("<b>", "\x1b[1;33m");
    // Replace closing tags with ANSI reset
    result = result.replace("</b>", "\x1b[0m");

    result
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        // Find a valid UTF-8 char boundary to avoid panic on multi-byte chars
        let mut end = max_len.saturating_sub(3);
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

fn parse_date_start(value: &str) -> Result<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("Invalid date format: {value}. Use YYYY-MM-DD."))?;
    let naive = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| anyhow::anyhow!("Invalid start-of-day for date: {value}"))?;
    Ok(DateTime::from_naive_utc_and_offset(naive, Utc))
}

fn parse_date_end(value: &str) -> Result<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("Invalid date format: {value}. Use YYYY-MM-DD."))?;
    let naive = date
        .and_hms_opt(23, 59, 59)
        .ok_or_else(|| anyhow::anyhow!("Invalid end-of-day for date: {value}"))?;
    Ok(DateTime::from_naive_utc_and_offset(naive, Utc))
}

fn is_reply(result: &SearchResult) -> bool {
    if result.result_type != SearchResultType::Tweet {
        return false;
    }
    result
        .metadata
        .get("in_reply_to")
        .and_then(|v| v.as_str())
        .is_some()
}

fn apply_search_filters(results: &mut Vec<SearchResult>, args: &cli::SearchArgs) -> Result<()> {
    let since = match args.since.as_deref() {
        Some(value) => Some(parse_date_start(value)?),
        None => None,
    };
    let until = match args.until.as_deref() {
        Some(value) => Some(parse_date_end(value)?),
        None => None,
    };

    if since.is_some() || until.is_some() {
        results.retain(|r| {
            if r.result_type != SearchResultType::Tweet {
                return true;
            }
            if let Some(since_dt) = since {
                if r.created_at < since_dt {
                    return false;
                }
            }
            if let Some(until_dt) = until {
                if r.created_at > until_dt {
                    return false;
                }
            }
            true
        });
    }

    if args.replies_only {
        results.retain(is_reply);
    } else if args.no_replies {
        results.retain(|r| !is_reply(r));
    }

    Ok(())
}

fn engagement_score(result: &SearchResult) -> i64 {
    if result.result_type != SearchResultType::Tweet {
        return 0;
    }
    let favs = result
        .metadata
        .get("favorite_count")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let rts = result
        .metadata
        .get("retweet_count")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    favs.saturating_add(rts)
}

fn apply_search_sort(results: &mut [SearchResult], sort: &SortOrder) {
    use std::cmp::Ordering;

    match sort {
        SortOrder::Relevance => {}
        SortOrder::Date => {
            results.sort_by(|a, b| {
                let cmp = a.created_at.cmp(&b.created_at);
                if cmp == Ordering::Equal {
                    b.score.total_cmp(&a.score)
                } else {
                    cmp
                }
            });
        }
        SortOrder::DateDesc => {
            results.sort_by(|a, b| {
                let cmp = b.created_at.cmp(&a.created_at);
                if cmp == Ordering::Equal {
                    b.score.total_cmp(&a.score)
                } else {
                    cmp
                }
            });
        }
        SortOrder::Engagement => {
            results.sort_by(|a, b| {
                let cmp = engagement_score(b).cmp(&engagement_score(a));
                if cmp == Ordering::Equal {
                    b.created_at.cmp(&a.created_at)
                } else {
                    cmp
                }
            });
        }
    }
}

fn validate_output_fields(fields: &[String]) -> Result<()> {
    const ALLOWED: [&str; 7] = [
        "result_type",
        "id",
        "text",
        "created_at",
        "score",
        "highlights",
        "metadata",
    ];
    for field in fields {
        if !ALLOWED.contains(&field.as_str()) {
            anyhow::bail!("Unknown field for --fields: {field}");
        }
    }
    Ok(())
}

fn filter_results_fields(
    results: &[SearchResult],
    fields: &[String],
) -> Result<Vec<serde_json::Value>> {
    let mut filtered = Vec::with_capacity(results.len());
    for result in results {
        let value = serde_json::to_value(result)?;
        let obj = value.as_object().ok_or_else(|| {
            anyhow::anyhow!("Failed to serialize search result for field filtering.")
        })?;
        let mut new_obj = serde_json::Map::new();
        for field in fields {
            if let Some(val) = obj.get(field) {
                new_obj.insert(field.clone(), val.clone());
            }
        }
        filtered.push(serde_json::Value::Object(new_obj));
    }
    Ok(filtered)
}

#[allow(clippy::too_many_lines)]
fn cmd_stats(cli: &Cli, args: &cli::StatsArgs) -> Result<()> {
    let db_path = get_db_path(cli);

    if !db_path.exists() {
        anyhow::bail!("No indexed archive found. Run 'xf index <archive_path>' first.");
    }

    let storage = Storage::open(&db_path)?;
    let stats = storage.get_stats()?;

    // --detailed shows all analytics (temporal + engagement + content)
    let show_temporal = args.temporal || args.detailed;
    let show_engagement = args.engagement || args.detailed;
    let show_content = args.content || args.detailed;

    // Show progress for large archives when computing detailed analytics
    if args.detailed && stats.tweets_count > 10_000 && !cli.quiet {
        eprintln!("Computing detailed analytics...");
    }

    // Temporal analytics uses efficient SQL aggregations
    let temporal = if show_temporal {
        Some(TemporalStats::compute(&storage)?)
    } else {
        None
    };

    // Engagement analytics
    let engagement = if show_engagement {
        Some(EngagementStats::compute(&storage, args.top)?)
    } else {
        None
    };

    // Content analytics - also provides top_hashtags and top_mentions efficiently
    let content = if show_content || args.hashtags || args.mentions {
        Some(ContentStats::compute(&storage, args.top)?)
    } else {
        None
    };

    // Extract top_hashtags/mentions from ContentStats if requested separately
    #[allow(clippy::cast_possible_truncation)]
    let top_hashtags = if args.hashtags && !show_content {
        content.as_ref().map(|c| {
            c.top_hashtags
                .iter()
                .map(|t| CountItem {
                    value: t.tag.clone(),
                    count: t.count as usize,
                })
                .collect::<Vec<_>>()
        })
    } else {
        None
    };

    #[allow(clippy::cast_possible_truncation)]
    let top_mentions = if args.mentions && !show_content {
        content.as_ref().map(|c| {
            c.top_mentions
                .iter()
                .map(|t| CountItem {
                    value: t.tag.clone(),
                    count: t.count as usize,
                })
                .collect::<Vec<_>>()
        })
    } else {
        None
    };

    let needs_extended =
        show_temporal || show_engagement || show_content || args.hashtags || args.mentions;

    // For backward compatibility with JSON output, include monthly breakdown in detailed
    let detailed = if args.detailed && temporal.is_some() {
        temporal
            .as_ref()
            .map(|t| build_monthly_counts_from_daily(&t.daily_counts))
    } else {
        None
    };

    match cli.format {
        OutputFormat::Json | OutputFormat::JsonPretty => {
            if needs_extended {
                let extended = StatsExtended {
                    stats,
                    detailed,
                    top_hashtags,
                    top_mentions,
                    temporal,
                    engagement,
                    content,
                };
                let json = if matches!(cli.format, OutputFormat::JsonPretty) {
                    serde_json::to_string_pretty(&extended)?
                } else {
                    serde_json::to_string(&extended)?
                };
                println!("{json}");
            } else {
                let json = if matches!(cli.format, OutputFormat::JsonPretty) {
                    serde_json::to_string_pretty(&stats)?
                } else {
                    serde_json::to_string(&stats)?
                };
                println!("{json}");
            }
        }
        _ => {
            // Show fancy banner for --detailed mode
            if args.detailed {
                println!("{}", "═".repeat(65).bright_blue());
                println!(
                    "{}",
                    "              ARCHIVE ANALYTICS DASHBOARD              "
                        .bold()
                        .on_bright_blue()
                );
                println!("{}", "═".repeat(65).bright_blue());
                println!();
            }

            println!("{}", "📊 Overview".bold().cyan());
            println!("{}", "─".repeat(40));
            println!(
                "  {:<20} {:>10}",
                "Tweets:",
                format_count(stats.tweets_count)
            );
            println!("  {:<20} {:>10}", "Likes:", format_count(stats.likes_count));
            println!(
                "  {:<20} {:>10}",
                "DM Conversations:",
                format_count(stats.dm_conversations_count)
            );
            println!(
                "  {:<20} {:>10}",
                "DM Messages:",
                format_count(stats.dms_count)
            );
            println!(
                "  {:<20} {:>10}",
                "Grok Messages:",
                format_count(stats.grok_messages_count)
            );
            println!(
                "  {:<20} {:>10}",
                "Followers:",
                format_count(stats.followers_count)
            );
            println!(
                "  {:<20} {:>10}",
                "Following:",
                format_count(stats.following_count)
            );
            println!(
                "  {:<20} {:>10}",
                "Blocks:",
                format_count(stats.blocks_count)
            );
            println!("  {:<20} {:>10}", "Mutes:", format_count(stats.mutes_count));
            println!("{}", "─".repeat(40));

            if let (Some(first), Some(last)) = (stats.first_tweet_date, stats.last_tweet_date) {
                println!(
                    "  First tweet: {}",
                    first.format("%Y-%m-%d").to_string().green()
                );
                println!(
                    "  Last tweet:  {}",
                    last.format("%Y-%m-%d").to_string().green()
                );
            }

            if let Some(detailed) = detailed {
                if !detailed.is_empty() {
                    println!();
                    println!("{}", "📅 Tweets by Month".bold().cyan());
                    println!("{}", "─".repeat(40));
                    for entry in detailed {
                        println!(
                            "  {:04}-{:02}: {}",
                            entry.year,
                            entry.month,
                            format_count(i64::try_from(entry.count).unwrap_or(i64::MAX))
                        );
                    }
                }
            }

            if let Some(items) = top_hashtags {
                if !items.is_empty() {
                    println!();
                    println!("{}", "#️⃣ Top Hashtags".bold().cyan());
                    println!("{}", "─".repeat(40));
                    for item in items {
                        println!(
                            "  {:<20} {}",
                            item.value,
                            format_count(i64::try_from(item.count).unwrap_or(i64::MAX))
                        );
                    }
                }
            }

            if let Some(items) = top_mentions {
                if !items.is_empty() {
                    println!();
                    println!("{}", "👤 Top Mentions".bold().cyan());
                    println!("{}", "─".repeat(40));
                    for item in items {
                        println!(
                            "  {:<20} {}",
                            item.value,
                            format_count(i64::try_from(item.count).unwrap_or(i64::MAX))
                        );
                    }
                }
            }

            #[allow(clippy::cast_possible_wrap)]
            if let Some(ref temporal) = temporal {
                println!();
                println!("{}", "📅 Temporal Patterns".bold().cyan());
                println!("{}", "─".repeat(60));

                // Activity sparkline
                let sparkline = stats_analytics::sparkline_from_daily(&temporal.daily_counts, 50);
                println!("  Activity: {}", sparkline.dimmed());

                // Key metrics
                println!(
                    "  {:<25} {:>10}",
                    "Active days:",
                    format_count(temporal.active_days_count as i64)
                );
                println!(
                    "  {:<25} {:>10}",
                    "Total days in range:",
                    format_count(temporal.total_days_in_range as i64)
                );
                println!(
                    "  {:<25} {:>10.1}",
                    "Avg tweets/active day:", temporal.avg_tweets_per_active_day
                );

                // Most active day
                if let Some(day) = temporal.most_active_day {
                    println!(
                        "  {:<25} {} ({})",
                        "Most active day:",
                        day.format("%Y-%m-%d").to_string().green(),
                        format_count(temporal.most_active_day_count as i64)
                    );
                }

                // Most active hour
                let hour_label = format!("{:02}:00", temporal.most_active_hour);
                println!(
                    "  {:<25} {} ({})",
                    "Most active hour:",
                    hour_label.green(),
                    format_count(temporal.most_active_hour_count as i64)
                );

                // Longest gap
                if temporal.longest_gap_days > 1 {
                    let gap_info = if let (Some(start), Some(end)) =
                        (temporal.longest_gap_start, temporal.longest_gap_end)
                    {
                        format!(
                            "{} days ({} to {})",
                            temporal.longest_gap_days,
                            start.format("%Y-%m-%d"),
                            end.format("%Y-%m-%d")
                        )
                    } else {
                        format!("{} days", temporal.longest_gap_days)
                    };
                    println!("  {:<25} {}", "Longest gap:", gap_info.yellow());
                }

                // Hourly distribution
                println!();
                println!("  {} (00-23):", "Hourly distribution".dimmed());
                let hourly_sparkline =
                    stats_analytics::format_hourly_sparkline(&temporal.hourly_distribution);
                println!("  {hourly_sparkline}");

                // Day of week distribution
                println!();
                println!("  {}:", "Day of week".dimmed());
                let dow_chart =
                    stats_analytics::format_dow_distribution(&temporal.dow_distribution);
                for line in dow_chart.lines() {
                    println!("  {line}");
                }
            }

            #[allow(clippy::cast_possible_wrap)]
            if let Some(ref engagement) = engagement {
                println!();
                println!("{}", "📈 Engagement Analytics".bold().cyan());
                println!("{}", "─".repeat(60));

                // Summary metrics
                println!(
                    "  Total Likes: {} | Total Retweets: {}",
                    format_count(engagement.total_likes as i64).green(),
                    format_count(engagement.total_retweets as i64).green()
                );
                println!(
                    "  Average per Tweet: {:.1} | Median: {}",
                    engagement.avg_engagement, engagement.median_engagement
                );

                // Trend sparkline
                if !engagement.monthly_trend.is_empty() {
                    println!();
                    println!("  {} (monthly avg):", "Engagement trend".dimmed());
                    let trend_sparkline =
                        stats_analytics::sparkline_from_monthly(&engagement.monthly_trend, 24);
                    println!("  {trend_sparkline}");
                }

                // Likes histogram
                println!();
                println!("  {}:", "Likes distribution".dimmed());
                let histogram =
                    stats_analytics::format_likes_histogram(&engagement.likes_histogram);
                for line in histogram.lines() {
                    println!("  {line}");
                }

                // Top performing tweets
                if !engagement.top_tweets.is_empty() {
                    println!();
                    println!("  {}:", "Top performing tweets".dimmed());
                    for (i, tweet) in engagement.top_tweets.iter().enumerate() {
                        println!(
                            "  {}. [{} {} {}] \"{}\" ({})",
                            i + 1,
                            format!("{}", tweet.likes).green(),
                            "♥".red(),
                            format!("{}", tweet.retweets).cyan(),
                            tweet.text_preview.dimmed(),
                            tweet.created_at.format("%b %d, %Y")
                        );
                    }
                }
            }

            #[allow(clippy::cast_possible_wrap)]
            if let Some(ref content) = content {
                println!();
                println!("{}", "📝 Content Analysis".bold().cyan());
                println!("{}", "─".repeat(60));

                // Content type ratios
                println!(
                    "  {:<25} {:>6.1}%",
                    "Tweets with media:", content.media_ratio
                );
                println!(
                    "  {:<25} {:>6.1}%",
                    "Tweets with links:", content.link_ratio
                );
                println!("  {:<25} {:>6.1}%", "Replies:", content.reply_ratio);
                println!(
                    "  {:<25} {:>10}",
                    "Self-threads:",
                    format_count(content.thread_count as i64)
                );
                println!(
                    "  {:<25} {:>10}",
                    "Standalone tweets:",
                    format_count(content.standalone_count as i64)
                );

                // Tweet length
                println!();
                println!(
                    "  {:<25} {:.1} chars",
                    "Average tweet length:", content.avg_tweet_length
                );
                println!();
                println!("  {}:", "Length distribution".dimmed());
                let length_chart =
                    stats_analytics::format_length_distribution(&content.length_distribution);
                for line in length_chart.lines() {
                    println!("  {line}");
                }

                // Top hashtags
                if !content.top_hashtags.is_empty() {
                    println!();
                    println!("  {}:", "Top hashtags".dimmed());
                    for tag in content.top_hashtags.iter().take(6) {
                        println!("    #{:<20} {}", tag.tag, format_count(tag.count as i64));
                    }
                }

                // Top mentions
                if !content.top_mentions.is_empty() {
                    println!();
                    println!("  {}:", "Top mentions".dimmed());
                    for mention in content.top_mentions.iter().take(6) {
                        println!(
                            "    @{:<20} {}",
                            mention.tag,
                            format_count(mention.count as i64)
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

#[derive(Serialize)]
struct StatsExtended {
    stats: ArchiveStats,
    #[serde(skip_serializing_if = "Option::is_none")]
    detailed: Option<Vec<StatsPeriod>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_hashtags: Option<Vec<CountItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_mentions: Option<Vec<CountItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temporal: Option<TemporalStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    engagement: Option<EngagementStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<ContentStats>,
}

#[derive(Serialize)]
struct StatsPeriod {
    year: i32,
    month: u32,
    count: usize,
}

#[derive(Serialize)]
struct CountItem {
    value: String,
    count: usize,
}

/// Build monthly counts from pre-computed daily counts (efficient SQL-based approach).
#[allow(clippy::cast_possible_truncation)]
fn build_monthly_counts_from_daily(
    daily_counts: &[stats_analytics::DailyCount],
) -> Vec<StatsPeriod> {
    use std::collections::BTreeMap;

    let mut counts: BTreeMap<(i32, u32), usize> = BTreeMap::new();
    for day in daily_counts {
        let key = (day.date.year(), day.date.month());
        *counts.entry(key).or_insert(0) += day.count as usize;
    }

    counts
        .into_iter()
        .map(|((year, month), count)| StatsPeriod { year, month, count })
        .collect()
}

fn format_count(n: i64) -> String {
    if n >= 1_000_000 {
        let whole = n / 1_000_000;
        let tenths = (n % 1_000_000) / 100_000;
        format!("{whole}.{tenths}M")
    } else if n >= 1_000 {
        let whole = n / 1_000;
        let tenths = (n % 1_000) / 100;
        format!("{whole}.{tenths}K")
    } else {
        n.to_string()
    }
}

fn cmd_tweet(cli: &Cli, args: &cli::TweetArgs) -> Result<()> {
    let db_path = get_db_path(cli);
    let storage = Storage::open(&db_path)?;

    if args.thread {
        return cmd_tweet_thread(cli, &storage, args);
    }

    let tweet = storage.get_tweet(&args.id)?;

    match tweet {
        Some(t) => match cli.format {
            OutputFormat::Json | OutputFormat::JsonPretty => {
                let json = if matches!(cli.format, OutputFormat::JsonPretty) {
                    serde_json::to_string_pretty(&t)?
                } else {
                    serde_json::to_string(&t)?
                };
                println!("{json}");
            }
            _ => {
                println!("{}", "─".repeat(60));
                println!("{}", t.full_text);
                println!("{}", "─".repeat(60));
                println!(
                    "  ID: {}  Date: {}",
                    t.id.dimmed(),
                    t.created_at.format("%Y-%m-%d %H:%M").to_string().dimmed()
                );
                if args.engagement {
                    println!(
                        "  {} likes  {} retweets",
                        t.favorite_count.to_string().cyan(),
                        t.retweet_count.to_string().cyan()
                    );
                }
                if !t.hashtags.is_empty() {
                    println!("  Hashtags: {}", t.hashtags.join(", ").blue());
                }
                if let Some(reply_to) = &t.in_reply_to_screen_name {
                    println!("  Reply to: @{}", reply_to.green());
                }
            }
        },
        None => {
            println!("{}", format!("Tweet {} not found.", args.id).red());
        }
    }

    Ok(())
}

#[allow(clippy::too_many_lines)]
fn cmd_list(cli: &Cli, args: &cli::ListArgs) -> Result<()> {
    let db_path = get_db_path(cli);

    if matches!(args.what, ListTarget::Files) {
        println!(
            "{}",
            "Use 'xf index <path>' to index an archive first, then use list commands to browse data."
                .yellow()
        );
        return Ok(());
    }

    if !db_path.exists() {
        anyhow::bail!("No indexed archive found. Run 'xf index <archive_path>' first.");
    }

    let storage = Storage::open(&db_path)?;
    let limit = Some(args.limit);

    match args.what {
        ListTarget::Files => unreachable!(),
        ListTarget::Tweets => {
            let tweets = storage.get_all_tweets(limit)?;
            println!(
                "{} {} tweets:\n",
                "Showing".dimmed(),
                tweets.len().to_string().cyan()
            );
            for tweet in &tweets {
                let date = tweet.created_at.format("%Y-%m-%d %H:%M").to_string();
                let text = truncate_text(&tweet.full_text, 80);
                println!("{} {} {}", date.dimmed(), tweet.id.cyan(), text);
            }
        }
        ListTarget::Likes => {
            let likes = storage.get_all_likes(limit)?;
            println!(
                "{} {} likes:\n",
                "Showing".dimmed(),
                likes.len().to_string().cyan()
            );
            for like in &likes {
                let text = like
                    .full_text
                    .as_ref()
                    .map_or_else(|| "[No text]".to_string(), |t| truncate_text(t, 80));
                println!("{} {}", like.tweet_id.cyan(), text);
            }
        }
        ListTarget::Dms | ListTarget::Conversations => {
            let dms = storage.get_all_dms(limit)?;
            println!(
                "{} {} DM messages:\n",
                "Showing".dimmed(),
                dms.len().to_string().cyan()
            );
            for dm in &dms {
                let date = dm.created_at.format("%Y-%m-%d %H:%M").to_string();
                let text = truncate_text(&dm.text, 60);
                println!(
                    "{} {} {} {} {}",
                    date.dimmed(),
                    dm.sender_id.green(),
                    "→".dimmed(),
                    dm.recipient_id.blue(),
                    text
                );
            }
        }
        ListTarget::Followers => {
            let followers = storage.get_all_followers(limit)?;
            println!(
                "{} {} followers:\n",
                "Showing".dimmed(),
                followers.len().to_string().cyan()
            );
            for follower in &followers {
                let link = follower.user_link.as_deref().unwrap_or("[no link]");
                println!("{} {}", follower.account_id.cyan(), link.dimmed());
            }
        }
        ListTarget::Following => {
            let following = storage.get_all_following(limit)?;
            println!(
                "{} {} following:\n",
                "Showing".dimmed(),
                following.len().to_string().cyan()
            );
            for f in &following {
                let link = f.user_link.as_deref().unwrap_or("[no link]");
                println!("{} {}", f.account_id.cyan(), link.dimmed());
            }
        }
        ListTarget::Blocks => {
            let blocks = storage.get_all_blocks(limit)?;
            println!(
                "{} {} blocks:\n",
                "Showing".dimmed(),
                blocks.len().to_string().cyan()
            );
            for block in &blocks {
                let link = block.user_link.as_deref().unwrap_or("[no link]");
                println!("{} {}", block.account_id.cyan(), link.dimmed());
            }
        }
        ListTarget::Mutes => {
            let mutes = storage.get_all_mutes(limit)?;
            println!(
                "{} {} mutes:\n",
                "Showing".dimmed(),
                mutes.len().to_string().cyan()
            );
            for mute in &mutes {
                let link = mute.user_link.as_deref().unwrap_or("[no link]");
                println!("{} {}", mute.account_id.cyan(), link.dimmed());
            }
        }
    }

    Ok(())
}

/// Truncate text to a maximum length, adding ellipsis if needed.
/// Uses character count, not byte count, to properly handle UTF-8.
fn truncate_text(text: &str, max_len: usize) -> String {
    let text = text.replace('\n', " ").replace('\r', "");
    let char_count = text.chars().count();
    if char_count <= max_len {
        text
    } else {
        let truncated: String = text.chars().take(max_len.saturating_sub(3)).collect();
        format!("{truncated}...")
    }
}

fn cmd_export(cli: &Cli, args: &cli::ExportArgs) -> Result<()> {
    let db_path = get_db_path(cli);

    if !db_path.exists() {
        anyhow::bail!("No indexed archive found. Run 'xf index <archive_path>' first.");
    }

    let storage = Storage::open(&db_path)?;

    // Build output based on target
    let output = match args.what {
        ExportTarget::Tweets => {
            let tweets = storage.get_all_tweets(args.limit)?;
            format_export(&tweets, &args.format)?
        }
        ExportTarget::Likes => {
            let likes = storage.get_all_likes(args.limit)?;
            format_export(&likes, &args.format)?
        }
        ExportTarget::Dms => {
            let dms = storage.get_all_dms(args.limit)?;
            format_export(&dms, &args.format)?
        }
        ExportTarget::Followers => {
            let followers = storage.get_all_followers(args.limit)?;
            format_export(&followers, &args.format)?
        }
        ExportTarget::Following => {
            let following = storage.get_all_following(args.limit)?;
            format_export(&following, &args.format)?
        }
        ExportTarget::All => {
            // For "all", we create a combined structure
            let tweets = storage.get_all_tweets(args.limit)?;
            let likes = storage.get_all_likes(args.limit)?;
            let dms = storage.get_all_dms(args.limit)?;
            let followers = storage.get_all_followers(args.limit)?;
            let following = storage.get_all_following(args.limit)?;

            match args.format {
                ExportFormat::Json => {
                    let combined = serde_json::json!({
                        "tweets": tweets,
                        "likes": likes,
                        "dms": dms,
                        "followers": followers,
                        "following": following
                    });
                    serde_json::to_string_pretty(&combined)?
                }
                ExportFormat::Jsonl => {
                    let mut jsonl_lines = Vec::new();
                    for t in &tweets {
                        jsonl_lines.push(format!(
                            r#"{{"type":"tweet","data":{}}}"#,
                            serde_json::to_string(t)?
                        ));
                    }
                    for l in &likes {
                        jsonl_lines.push(format!(
                            r#"{{"type":"like","data":{}}}"#,
                            serde_json::to_string(l)?
                        ));
                    }
                    for d in &dms {
                        jsonl_lines.push(format!(
                            r#"{{"type":"dm","data":{}}}"#,
                            serde_json::to_string(d)?
                        ));
                    }
                    for f in &followers {
                        jsonl_lines.push(format!(
                            r#"{{"type":"follower","data":{}}}"#,
                            serde_json::to_string(f)?
                        ));
                    }
                    for f in &following {
                        jsonl_lines.push(format!(
                            r#"{{"type":"following","data":{}}}"#,
                            serde_json::to_string(f)?
                        ));
                    }
                    jsonl_lines.join("\n")
                }
                ExportFormat::Csv => {
                    anyhow::bail!(
                        "CSV export not supported for 'all' target. Export individual types instead."
                    );
                }
            }
        }
    };

    // Write to file or stdout
    if let Some(path) = &args.output {
        std::fs::write(path, &output)?;
        println!(
            "{} Exported to {}",
            "✓".green(),
            path.display().to_string().cyan()
        );
    } else {
        println!("{output}");
    }

    Ok(())
}

/// Format data for export based on the specified format
fn format_export<T: serde::Serialize>(data: &[T], format: &ExportFormat) -> Result<String> {
    match format {
        ExportFormat::Json => Ok(serde_json::to_string_pretty(data)?),
        ExportFormat::Jsonl => {
            let lines: Vec<String> = data
                .iter()
                .map(|item| serde_json::to_string(item))
                .collect::<std::result::Result<_, _>>()?;
            Ok(lines.join("\n"))
        }
        ExportFormat::Csv => {
            if data.is_empty() {
                return Ok(String::new());
            }
            // Use serde_json to get a consistent field representation
            let first = serde_json::to_value(&data[0])?;
            if let serde_json::Value::Object(map) = first {
                let headers: Vec<&str> = map.keys().map(String::as_str).collect();
                let mut output = headers.join(",");
                output.push('\n');

                for item in data {
                    let val = serde_json::to_value(item)?;
                    if let serde_json::Value::Object(obj) = val {
                        let row: Vec<String> = headers
                            .iter()
                            .map(|&h| obj.get(h).map(csv_escape).unwrap_or_default())
                            .collect();
                        output.push_str(&row.join(","));
                        output.push('\n');
                    }
                }
                Ok(output)
            } else {
                anyhow::bail!("Data structure not suitable for CSV export");
            }
        }
    }
}

/// Escape a JSON value for CSV output
fn csv_escape(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => {
            // Quote if contains comma, quote, newline, or carriage return per RFC 4180
            if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.clone()
            }
        }
        serde_json::Value::Array(arr) => {
            let inner = serde_json::to_string(arr).unwrap_or_default();
            format!("\"{}\"", inner.replace('"', "\"\""))
        }
        serde_json::Value::Object(obj) => {
            let inner = serde_json::to_string(obj).unwrap_or_default();
            format!("\"{}\"", inner.replace('"', "\"\""))
        }
    }
}

fn cmd_config(cli: &Cli, args: &cli::ConfigArgs) -> Result<()> {
    let mut config = Config::load();
    let set_present = args.set.is_some();
    let archive_present = args.archive.is_some();

    if let Some(set) = &args.set {
        apply_config_set(&mut config, set)?;
    }

    if let Some(archive) = &args.archive {
        config.paths.archive = Some(archive.clone());
    }

    if set_present || archive_present {
        config
            .save()
            .with_context(|| "Failed to save config file".to_string())?;
        println!("{}", "✓ Updated configuration".green());
    }
    if args.show {
        println!("{}", "Current Configuration".bold().cyan());
        println!("  Database: {}", get_db_path(cli).display());
        println!("  Index: {}", get_index_path(cli).display());
        if let Some(archive) = &config.paths.archive {
            println!("  Archive: {}", archive.display());
        }
    }
    Ok(())
}

fn cmd_tweet_thread(cli: &Cli, storage: &Storage, args: &cli::TweetArgs) -> Result<()> {
    let thread = storage.get_tweet_thread(&args.id)?;

    if thread.is_empty() {
        println!("{}", format!("Tweet {} not found.", args.id).red());
        return Ok(());
    }

    match cli.format {
        OutputFormat::Json | OutputFormat::JsonPretty => {
            let json = if matches!(cli.format, OutputFormat::JsonPretty) {
                serde_json::to_string_pretty(&thread)?
            } else {
                serde_json::to_string(&thread)?
            };
            println!("{json}");
        }
        _ => {
            println!("{}", "Thread".bold().cyan());
            println!("{}", "─".repeat(60));
            for tweet in &thread {
                let date = tweet.created_at.format("%Y-%m-%d %H:%M").to_string();
                let text = truncate_text(&tweet.full_text, 100);
                println!("{} {} {}", date.dimmed(), tweet.id.cyan(), text);
                if args.engagement {
                    println!(
                        "  {} likes  {} retweets",
                        tweet.favorite_count.to_string().cyan(),
                        tweet.retweet_count.to_string().cyan()
                    );
                }
            }
        }
    }

    Ok(())
}

fn apply_config_set(config: &mut Config, raw: &str) -> Result<()> {
    let (key, value) = raw
        .split_once('=')
        .map(|(k, v)| (k.trim(), v.trim()))
        .ok_or_else(|| anyhow::anyhow!("Invalid --set format. Use key=value."))?;

    if key.is_empty() {
        anyhow::bail!("Invalid --set key. Use key=value.");
    }

    match key {
        "db" | "paths.db" => {
            config.paths.db = parse_optional_path(value);
        }
        "index" | "paths.index" => {
            config.paths.index = parse_optional_path(value);
        }
        "archive" | "paths.archive" => {
            config.paths.archive = parse_optional_path(value);
        }
        "search.default_limit" => {
            config.search.default_limit = parse_usize(value, key)?;
        }
        "search.highlight" => {
            config.search.highlight = parse_bool(value, key)?;
        }
        "search.fuzzy" => {
            config.search.fuzzy = parse_bool(value, key)?;
        }
        "search.min_score" => {
            let parsed = parse_f32(value, key)?;
            if !(0.0..=1.0).contains(&parsed) {
                anyhow::bail!("{key} must be between 0.0 and 1.0.");
            }
            config.search.min_score = parsed;
        }
        "search.cache_size" => {
            config.search.cache_size = parse_usize(value, key)?;
        }
        "indexing.parallel" => {
            config.indexing.parallel = parse_bool(value, key)?;
        }
        "indexing.buffer_size_mb" => {
            config.indexing.buffer_size_mb = parse_usize(value, key)?;
        }
        "indexing.threads" => {
            config.indexing.threads = parse_usize(value, key)?;
        }
        "indexing.skip_types" => {
            config.indexing.skip_types = parse_csv_list(value);
        }
        "output.format" => {
            if value.is_empty() {
                anyhow::bail!("output.format cannot be empty.");
            }
            config.output.format = value.to_string();
        }
        "output.colors" => {
            config.output.colors = parse_bool(value, key)?;
        }
        "output.quiet" => {
            config.output.quiet = parse_bool(value, key)?;
        }
        "output.timings" => {
            config.output.timings = parse_bool(value, key)?;
        }
        _ => {
            anyhow::bail!("Unknown config key: {key}");
        }
    }

    Ok(())
}

fn parse_optional_path(value: &str) -> Option<PathBuf> {
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

fn parse_bool(value: &str, key: &str) -> Result<bool> {
    match value.to_lowercase().as_str() {
        "true" | "1" | "yes" | "y" | "on" => Ok(true),
        "false" | "0" | "no" | "n" | "off" => Ok(false),
        _ => anyhow::bail!("Invalid boolean value for {key}: {value}"),
    }
}

fn parse_usize(value: &str, key: &str) -> Result<usize> {
    value
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid integer value for {key}: {value}"))
}

fn parse_f32(value: &str, key: &str) -> Result<f32> {
    value
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid float value for {key}: {value}"))
}

fn parse_csv_list(value: &str) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    value
        .split(',')
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .collect()
}

fn cmd_update() {
    println!("{}", "Checking for updates...".cyan());
    println!(
        "To update, run:\n  {}",
        "curl -fsSL https://raw.githubusercontent.com/Dicklesworthstone/xf/main/install.sh | bash"
            .bold()
    );
}

fn cmd_completions(args: &cli::CompletionsArgs) {
    let mut cmd = Cli::command();
    generate(args.shell, &mut cmd, "xf", &mut io::stdout());
}
