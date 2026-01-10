//! xf - Ultra-fast X data archive search CLI
//!
//! Main entry point for the xf command-line tool.

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, Write};
use std::path::PathBuf;
use tracing::{debug, error, info, warn, Level};
use tracing_subscriber::EnvFilter;

use xf::*;

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
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive(log_level.into())
        )
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
        Commands::Update => cmd_update(),
        Commands::Completions(args) => cmd_completions(args.clone()),
    }
}

fn get_db_path(cli: &Cli) -> PathBuf {
    cli.db.clone().unwrap_or_else(default_db_path)
}

fn get_index_path(cli: &Cli) -> PathBuf {
    cli.index.clone().unwrap_or_else(default_index_path)
}

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
    let data_types = if let Some(only) = &args.only {
        only.clone()
    } else if let Some(skip) = &args.skip {
        DataType::all()
            .into_iter()
            .filter(|t| !skip.contains(t))
            .collect()
    } else {
        DataType::all()
    };

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
                pb.println(format!("  {} {} Grok messages", "✓".green(), messages.len()));
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

    let search_engine = SearchEngine::open(&index_path)?;
    let storage = Storage::open(&db_path)?;

    // Convert data types to search doc types
    let doc_types: Option<Vec<search::DocType>> = args.types.as_ref().map(|types| {
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
    });

    let results = search_engine.search(
        &args.query,
        doc_types.as_deref(),
        args.limit + args.offset,
    )?;

    // Apply offset
    let results: Vec<_> = results.into_iter().skip(args.offset).collect();

    if results.is_empty() {
        println!("{}", "No results found.".yellow());
        return Ok(());
    }

    // Output results
    match cli.format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&results)?);
        }
        OutputFormat::JsonPretty => {
            println!("{}", serde_json::to_string_pretty(&results)?);
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

    // Word wrap the text
    let wrapped = textwrap::wrap(&result.text, 78);
    for line in wrapped {
        println!("   {}", line);
    }

    if result.created_at.timestamp() > 0 {
        println!(
            "   {}",
            result.created_at.format("%Y-%m-%d %H:%M").to_string().dimmed()
        );
    }

    println!();
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

fn cmd_stats(cli: &Cli, args: &cli::StatsArgs) -> Result<()> {
    let db_path = get_db_path(cli);

    if !db_path.exists() {
        anyhow::bail!(
            "No indexed archive found. Run 'xf index <archive_path>' first."
        );
    }

    let storage = Storage::open(&db_path)?;
    let stats = storage.get_stats()?;

    match cli.format {
        OutputFormat::Json | OutputFormat::JsonPretty => {
            let json = if matches!(cli.format, OutputFormat::JsonPretty) {
                serde_json::to_string_pretty(&stats)?
            } else {
                serde_json::to_string(&stats)?
            };
            println!("{}", json);
        }
        _ => {
            println!("{}", "Archive Statistics".bold().cyan());
            println!("{}", "─".repeat(40));
            println!("  {:<20} {:>10}", "Tweets:", format_count(stats.tweets_count));
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
            println!("  {:<20} {:>10}", "Followers:", format_count(stats.followers_count));
            println!("  {:<20} {:>10}", "Following:", format_count(stats.following_count));
            println!("  {:<20} {:>10}", "Blocks:", format_count(stats.blocks_count));
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
        }
    }

    Ok(())
}

fn format_count(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn cmd_tweet(cli: &Cli, args: &cli::TweetArgs) -> Result<()> {
    let db_path = get_db_path(cli);
    let storage = Storage::open(&db_path)?;

    let tweet = storage.get_tweet(&args.id)?;

    match tweet {
        Some(t) => {
            match cli.format {
                OutputFormat::Json | OutputFormat::JsonPretty => {
                    let json = if matches!(cli.format, OutputFormat::JsonPretty) {
                        serde_json::to_string_pretty(&t)?
                    } else {
                        serde_json::to_string(&t)?
                    };
                    println!("{}", json);
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
            }
        }
        None => {
            println!("{}", format!("Tweet {} not found.", args.id).red());
        }
    }

    Ok(())
}

fn cmd_list(cli: &Cli, args: &cli::ListArgs) -> Result<()> {
    let db_path = get_db_path(cli);

    match args.what {
        ListTarget::Files => {
            // List data files in archive - need archive path
            println!("{}", "Use 'xf index <path>' to index an archive first.".yellow());
        }
        _ => {
            if !db_path.exists() {
                anyhow::bail!("No indexed archive found.");
            }
            println!("{}", "List command not fully implemented yet.".yellow());
        }
    }

    Ok(())
}

fn cmd_export(cli: &Cli, args: &cli::ExportArgs) -> Result<()> {
    println!("{}", "Export command not fully implemented yet.".yellow());
    Ok(())
}

fn cmd_config(cli: &Cli, args: &cli::ConfigArgs) -> Result<()> {
    if args.show {
        println!("{}", "Current Configuration".bold().cyan());
        println!("  Database: {}", get_db_path(cli).display());
        println!("  Index: {}", get_index_path(cli).display());
    }
    Ok(())
}

fn cmd_update() -> Result<()> {
    println!("{}", "Checking for updates...".cyan());
    println!(
        "To update, run:\n  {}",
        "curl -fsSL https://raw.githubusercontent.com/Dicklesworthstone/xf/main/install.sh | bash"
            .bold()
    );
    Ok(())
}

fn cmd_completions(args: cli::CompletionsArgs) -> Result<()> {
    let mut cmd = Cli::command();
    generate(args.shell, &mut cmd, "xf", &mut io::stdout());
    Ok(())
}
