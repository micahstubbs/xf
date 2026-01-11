//! Interactive REPL for xf.
//!
//! Provides a command-driven shell with history, basic search, and help.

use anyhow::{Context, Result};
use colored::Colorize;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::{CompletionType, Config, EditMode, Editor};
use std::path::PathBuf;
use tracing::{debug, info, warn};

use crate::{SearchEngine, SearchResult, Storage};

/// REPL session state.
pub struct ReplSession {
    storage: Storage,
    search: SearchEngine,
    last_results: Vec<SearchResult>,
    last_query: Option<String>,
    history_path: PathBuf,
    prompt_context: PromptContext,
    /// Current offset for pagination
    current_offset: usize,
    /// Page size for results
    page_size: usize,
}

#[derive(Default)]
#[allow(dead_code)]
enum PromptContext {
    #[default]
    Normal,
    WithResults(usize),
    InConversation(String),
}

#[derive(Debug)]
enum Command {
    Search { query: String },
    List { target: ListTarget },
    Refine { filter: String },
    More,
    Show { index: usize },
    Export { format: ExportFormat },
    Stats,
    Help { command: Option<String> },
    Quit,
}

#[derive(Debug, Clone, Copy)]
enum ListTarget {
    Tweets,
    Likes,
    Dms,
    Conversations,
    Followers,
    Following,
    Blocks,
    Mutes,
}

#[derive(Debug, Clone, Copy)]
enum ExportFormat {
    Json,
    Csv,
}

/// Run the REPL session.
///
/// # Errors
///
/// Returns an error if readline setup, history persistence, or command execution fails.
pub fn run(storage: Storage, search: SearchEngine) -> Result<()> {
    let config = Config::builder()
        .history_ignore_space(true)
        .history_ignore_dups(true)?
        .completion_type(CompletionType::List)
        .edit_mode(EditMode::Emacs)
        .build();

    let mut rl: Editor<(), DefaultHistory> = Editor::with_config(config)?;

    let history_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".xf_history");

    let mut session = ReplSession {
        storage,
        search,
        last_results: Vec::new(),
        last_query: None,
        history_path,
        prompt_context: PromptContext::Normal,
        current_offset: 0,
        page_size: 10,
    };

    let _ = rl.load_history(&session.history_path);

    info!("Starting xf REPL session");
    println!(
        "{}",
        "xf interactive mode. Type 'help' for commands, 'quit' to exit.".cyan()
    );
    println!();

    loop {
        let prompt = session.format_prompt();
        match rl.readline(&prompt) {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                if !matches!(line, "quit" | "exit" | "q") {
                    rl.add_history_entry(line)?;
                }

                debug!(command = %line, "REPL command");
                match session.execute(line) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => {
                        warn!(error = %e, "REPL command failed");
                        eprintln!("{}: {e}", "Error".red());
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
            }
            Err(ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(e) => {
                return Err(anyhow::anyhow!(e)).context("Readline failed");
            }
        }
    }

    rl.save_history(&session.history_path)?;
    info!("Ended xf REPL session");
    println!("Goodbye!");
    Ok(())
}

impl ReplSession {
    fn format_prompt(&self) -> String {
        match &self.prompt_context {
            PromptContext::Normal => "xf> ".to_string(),
            PromptContext::WithResults(n) => format!("xf [{n}]> "),
            PromptContext::InConversation(id) => {
                let snippet = id.get(..8.min(id.len())).unwrap_or(id);
                format!("xf [dm:{snippet}]> ")
            }
        }
    }

    fn execute(&mut self, input: &str) -> Result<bool> {
        let command = parse_command(input)?;
        match command {
            Command::Search { query } => {
                self.run_search(&query)?;
            }
            Command::List { target } => {
                self.run_list(target)?;
            }
            Command::Refine { filter } => {
                self.run_refine(&filter)?;
            }
            Command::More => {
                self.run_more()?;
            }
            Command::Show { index } => {
                self.run_show(index)?;
            }
            Command::Export { format } => {
                self.run_export(format)?;
            }
            Command::Stats => {
                self.run_stats()?;
            }
            Command::Help { command } => {
                print_help(command.as_deref());
            }
            Command::Quit => return Ok(false),
        }
        Ok(true)
    }

    fn run_search(&mut self, query: &str) -> Result<()> {
        let results = self.search.search(query, None, 100)?;
        let count = results.len();
        self.last_results = results;
        self.last_query = Some(query.to_string());
        self.current_offset = 0;
        self.prompt_context = PromptContext::WithResults(count);

        println!("{} {}", count.to_string().cyan(), "results".dimmed());
        print_results(&self.last_results, 0, self.page_size);
        Ok(())
    }

    fn run_stats(&self) -> Result<()> {
        let stats = self.storage.get_stats()?;
        println!("{}", "Archive Statistics".bold().cyan());
        println!("{}", "─".repeat(40));
        println!("  {:<20} {}", "Tweets:", stats.tweets_count);
        println!("  {:<20} {}", "Likes:", stats.likes_count);
        println!("  {:<20} {}", "DM Messages:", stats.dms_count);
        println!(
            "  {:<20} {}",
            "DM Conversations:", stats.dm_conversations_count
        );
        println!("  {:<20} {}", "Grok Messages:", stats.grok_messages_count);
        println!("  {:<20} {}", "Followers:", stats.followers_count);
        println!("  {:<20} {}", "Following:", stats.following_count);
        println!("  {:<20} {}", "Blocks:", stats.blocks_count);
        println!("  {:<20} {}", "Mutes:", stats.mutes_count);
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn run_list(&self, target: ListTarget) -> Result<()> {
        debug!(?target, "Listing items");
        match target {
            ListTarget::Tweets => {
                let tweets = self.storage.get_all_tweets(None)?;
                println!("{} {}", tweets.len().to_string().cyan(), "tweets".dimmed());
                for tweet in tweets.iter().take(self.page_size) {
                    let text = truncate_text(&tweet.full_text, 60);
                    println!(
                        "  {} {}",
                        tweet.created_at.format("%Y-%m-%d").to_string().dimmed(),
                        text
                    );
                }
                if tweets.len() > self.page_size {
                    println!(
                        "{}",
                        format!("… {} more", tweets.len() - self.page_size).dimmed()
                    );
                }
            }
            ListTarget::Likes => {
                let likes = self.storage.get_all_likes(None)?;
                println!("{} {}", likes.len().to_string().cyan(), "likes".dimmed());
                for like in likes.iter().take(self.page_size) {
                    let text = like.full_text.as_deref().unwrap_or("[no text]");
                    let text = truncate_text(text, 60);
                    println!("  {text}");
                }
                if likes.len() > self.page_size {
                    println!(
                        "{}",
                        format!("… {} more", likes.len() - self.page_size).dimmed()
                    );
                }
            }
            ListTarget::Dms => {
                let dms = self.storage.get_all_dms(None)?;
                println!(
                    "{} {}",
                    dms.len().to_string().cyan(),
                    "DM messages".dimmed()
                );
                for dm in dms.iter().take(self.page_size) {
                    let text = truncate_text(&dm.text, 60);
                    println!(
                        "  {} {}",
                        dm.created_at.format("%Y-%m-%d").to_string().dimmed(),
                        text
                    );
                }
                if dms.len() > self.page_size {
                    println!(
                        "{}",
                        format!("… {} more", dms.len() - self.page_size).dimmed()
                    );
                }
            }
            ListTarget::Conversations => {
                let stats = self.storage.get_stats()?;
                println!(
                    "{} {}",
                    stats.dm_conversations_count.to_string().cyan(),
                    "DM conversations".dimmed()
                );
            }
            ListTarget::Followers => {
                let followers = self.storage.get_all_followers(None)?;
                println!(
                    "{} {}",
                    followers.len().to_string().cyan(),
                    "followers".dimmed()
                );
                for f in followers.iter().take(self.page_size) {
                    println!("  {}", f.account_id);
                }
                if followers.len() > self.page_size {
                    println!(
                        "{}",
                        format!("… {} more", followers.len() - self.page_size).dimmed()
                    );
                }
            }
            ListTarget::Following => {
                let following = self.storage.get_all_following(None)?;
                println!(
                    "{} {}",
                    following.len().to_string().cyan(),
                    "following".dimmed()
                );
                for f in following.iter().take(self.page_size) {
                    println!("  {}", f.account_id);
                }
                if following.len() > self.page_size {
                    println!(
                        "{}",
                        format!("… {} more", following.len() - self.page_size).dimmed()
                    );
                }
            }
            ListTarget::Blocks => {
                let blocks = self.storage.get_all_blocks(None)?;
                println!(
                    "{} {}",
                    blocks.len().to_string().cyan(),
                    "blocked accounts".dimmed()
                );
                for b in blocks.iter().take(self.page_size) {
                    println!("  {}", b.account_id);
                }
                if blocks.len() > self.page_size {
                    println!(
                        "{}",
                        format!("… {} more", blocks.len() - self.page_size).dimmed()
                    );
                }
            }
            ListTarget::Mutes => {
                let mutes = self.storage.get_all_mutes(None)?;
                println!(
                    "{} {}",
                    mutes.len().to_string().cyan(),
                    "muted accounts".dimmed()
                );
                for m in mutes.iter().take(self.page_size) {
                    println!("  {}", m.account_id);
                }
                if mutes.len() > self.page_size {
                    println!(
                        "{}",
                        format!("… {} more", mutes.len() - self.page_size).dimmed()
                    );
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::unnecessary_wraps)] // Consistent return type with other run_* methods
    fn run_refine(&mut self, filter: &str) -> Result<()> {
        if self.last_results.is_empty() {
            warn!("No cached results to refine");
            println!("{}", "No results to refine. Run a search first.".yellow());
            return Ok(());
        }

        let filter_lower = filter.to_lowercase();
        let filtered: Vec<SearchResult> = self
            .last_results
            .iter()
            .filter(|r| r.text.to_lowercase().contains(&filter_lower))
            .cloned()
            .collect();

        let count = filtered.len();
        debug!(original = self.last_results.len(), filtered = count, filter = %filter, "Refined results");

        self.last_results = filtered;
        self.current_offset = 0;
        self.prompt_context = PromptContext::WithResults(count);

        println!(
            "{} {} (filtered by '{}')",
            count.to_string().cyan(),
            "results".dimmed(),
            filter.yellow()
        );
        print_results(&self.last_results, 0, self.page_size);
        Ok(())
    }

    #[allow(clippy::unnecessary_wraps)] // Consistent return type with other run_* methods
    fn run_more(&mut self) -> Result<()> {
        if self.last_results.is_empty() {
            println!("{}", "No results. Run a search first.".yellow());
            return Ok(());
        }

        let total = self.last_results.len();
        let new_offset = self.current_offset + self.page_size;

        if new_offset >= total {
            println!("{}", "No more results.".dimmed());
            return Ok(());
        }

        self.current_offset = new_offset;
        debug!(offset = new_offset, total, "Showing more results");

        print_results(&self.last_results, self.current_offset, self.page_size);
        println!(
            "{}",
            format!(
                "Showing {}-{} of {}",
                self.current_offset + 1,
                (self.current_offset + self.page_size).min(total),
                total
            )
            .dimmed()
        );
        Ok(())
    }

    #[allow(clippy::unnecessary_wraps)] // Consistent return type with other run_* methods
    fn run_show(&self, index: usize) -> Result<()> {
        if self.last_results.is_empty() {
            println!("{}", "No results. Run a search first.".yellow());
            return Ok(());
        }

        if index == 0 || index > self.last_results.len() {
            println!(
                "{}",
                format!("Invalid index. Use 1-{}.", self.last_results.len()).red()
            );
            return Ok(());
        }

        let result = &self.last_results[index - 1];
        debug!(index, result_type = %result.result_type, "Showing result details");

        println!("{}", "─".repeat(60));
        println!("{}: {}", "Type".cyan(), result.result_type);
        println!("{}: {}", "ID".cyan(), result.id);
        println!(
            "{}: {}",
            "Date".cyan(),
            result.created_at.format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!("{}: {:.2}", "Score".cyan(), result.score);
        println!();
        println!("{}", result.text);
        println!("{}", "─".repeat(60));
        Ok(())
    }

    fn run_export(&self, format: ExportFormat) -> Result<()> {
        if self.last_results.is_empty() {
            println!("{}", "No results to export. Run a search first.".yellow());
            return Ok(());
        }

        debug!(
            ?format,
            count = self.last_results.len(),
            "Exporting results"
        );

        match format {
            ExportFormat::Json => {
                let json = serde_json::to_string_pretty(&self.last_results)?;
                println!("{json}");
            }
            ExportFormat::Csv => {
                println!("id,type,score,created_at,text");
                for r in &self.last_results {
                    let created = r.created_at.to_rfc3339();
                    let text_escaped = r.text.replace('"', "\"\"").replace('\n', " ");
                    println!(
                        "{},{},{:.2},{},\"{}\"",
                        r.id, r.result_type, r.score, created, text_escaped
                    );
                }
            }
        }

        println!(
            "{}",
            format!("Exported {} results", self.last_results.len()).dimmed()
        );
        Ok(())
    }
}

fn parse_command(input: &str) -> Result<Command> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.is_empty() {
        anyhow::bail!("Empty command");
    }

    match parts[0] {
        "search" | "s" => {
            let query = parts[1..].join(" ");
            if query.is_empty() {
                anyhow::bail!("Search query cannot be empty.");
            }
            Ok(Command::Search { query })
        }
        "list" | "l" => {
            let target = parts.get(1).copied().unwrap_or("tweets");
            let target = parse_list_target(target)?;
            Ok(Command::List { target })
        }
        "refine" | "r" => {
            let filter = parts[1..].join(" ");
            if filter.is_empty() {
                anyhow::bail!("Refine requires a filter term.");
            }
            Ok(Command::Refine { filter })
        }
        "more" | "m" => Ok(Command::More),
        "show" => {
            let idx_str = parts
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("Usage: show <number>"))?;
            let index: usize = idx_str
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid number: {idx_str}"))?;
            Ok(Command::Show { index })
        }
        "export" | "e" => {
            let fmt = parts.get(1).copied().unwrap_or("json");
            let format = match fmt {
                "json" => ExportFormat::Json,
                "csv" => ExportFormat::Csv,
                _ => anyhow::bail!("Unknown export format: {fmt}. Use 'json' or 'csv'."),
            };
            Ok(Command::Export { format })
        }
        "stats" => Ok(Command::Stats),
        "help" | "h" | "?" => Ok(Command::Help {
            command: parts.get(1).map(ToString::to_string),
        }),
        "quit" | "exit" | "q" => Ok(Command::Quit),
        _ => anyhow::bail!(
            "Unknown command: {}. Type 'help' for available commands.",
            parts[0]
        ),
    }
}

fn parse_list_target(s: &str) -> Result<ListTarget> {
    match s {
        "tweets" | "t" => Ok(ListTarget::Tweets),
        "likes" | "favorites" => Ok(ListTarget::Likes),
        "dms" | "dm" | "messages" => Ok(ListTarget::Dms),
        "conversations" | "convos" => Ok(ListTarget::Conversations),
        "followers" => Ok(ListTarget::Followers),
        "following" => Ok(ListTarget::Following),
        "blocks" | "blocked" => Ok(ListTarget::Blocks),
        "mutes" | "muted" => Ok(ListTarget::Mutes),
        _ => anyhow::bail!(
            "Unknown list target: {s}. Options: tweets, likes, dms, conversations, followers, following, blocks, mutes"
        ),
    }
}

fn print_results(results: &[SearchResult], offset: usize, page_size: usize) {
    for (idx, result) in results.iter().skip(offset).take(page_size).enumerate() {
        let text = truncate_text(&result.text, 80);
        println!(
            "{:>3}. [{}] {}",
            offset + idx + 1,
            result.result_type.to_string().cyan(),
            text
        );
    }
    let remaining = results.len().saturating_sub(offset + page_size);
    if remaining > 0 {
        println!(
            "{}",
            format!("… {remaining} more results available (type 'more')").dimmed()
        );
    }
}

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

fn print_help(command: Option<&str>) {
    match command {
        Some("search" | "s") => {
            println!("{}", "search <query>".cyan());
            println!("  Search all indexed content (tweets, DMs, likes, Grok)");
            println!("  Aliases: s");
            println!("  Example: search hello world");
        }
        Some("list" | "l") => {
            println!("{}", "list [target]".cyan());
            println!("  List items from the archive");
            println!("  Aliases: l");
            println!(
                "  Targets: tweets, likes, dms, conversations, followers, following, blocks, mutes"
            );
            println!("  Example: list tweets");
        }
        Some("refine" | "r") => {
            println!("{}", "refine <filter>".cyan());
            println!("  Filter the current search results by a term");
            println!("  Aliases: r");
            println!("  Example: refine hello");
        }
        Some("more" | "m") => {
            println!("{}", "more".cyan());
            println!("  Show the next page of results");
            println!("  Aliases: m");
        }
        Some("show") => {
            println!("{}", "show <number>".cyan());
            println!("  Show full details of a result by its number");
            println!("  Example: show 1");
        }
        Some("export" | "e") => {
            println!("{}", "export [format]".cyan());
            println!("  Export current search results");
            println!("  Aliases: e");
            println!("  Formats: json (default), csv");
            println!("  Example: export csv");
        }
        Some("stats") => {
            println!("{}", "stats".cyan());
            println!("  Show archive statistics");
        }
        Some("quit" | "exit" | "q") => {
            println!("{}", "quit".cyan());
            println!("  Exit the REPL");
            println!("  Aliases: exit, q");
        }
        Some("help" | "h" | "?") => {
            println!("{}", "help [command]".cyan());
            println!("  Show help for all commands or a specific command");
            println!("  Aliases: h, ?");
        }
        _ => {
            println!("{}", "Commands:".bold().cyan());
            println!("  search <query>  - search indexed content (s)");
            println!("  list [target]   - list tweets, likes, dms, etc (l)");
            println!("  refine <filter> - filter current results (r)");
            println!("  more            - show next page of results (m)");
            println!("  show <number>   - show full result details");
            println!("  export [format] - export results as json/csv (e)");
            println!("  stats           - show archive statistics");
            println!("  help [command]  - show help (h, ?)");
            println!("  quit            - exit (exit, q)");
            println!();
            println!("{}", "Tips:".dimmed());
            println!("  {} Use Ctrl+C to cancel, Ctrl+D to quit", "•".dimmed());
            println!("  {} Arrow keys for history navigation", "•".dimmed());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ======================== Command Parsing Tests ========================

    #[test]
    fn test_parse_search_command() {
        let cmd = parse_command("search hello world").unwrap();
        assert!(matches!(cmd, Command::Search { query } if query == "hello world"));
    }

    #[test]
    fn test_parse_search_alias() {
        let cmd = parse_command("s rust programming").unwrap();
        assert!(matches!(cmd, Command::Search { query } if query == "rust programming"));
    }

    #[test]
    fn test_parse_search_empty_query_fails() {
        let result = parse_command("search");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_list_default_target() {
        let cmd = parse_command("list").unwrap();
        assert!(matches!(
            cmd,
            Command::List {
                target: ListTarget::Tweets
            }
        ));
    }

    #[test]
    fn test_parse_list_with_target() {
        let cmd = parse_command("list likes").unwrap();
        assert!(matches!(
            cmd,
            Command::List {
                target: ListTarget::Likes
            }
        ));
    }

    #[test]
    fn test_parse_list_alias() {
        let cmd = parse_command("l dms").unwrap();
        assert!(matches!(
            cmd,
            Command::List {
                target: ListTarget::Dms
            }
        ));
    }

    #[test]
    fn test_parse_list_target_aliases() {
        // Test various target aliases
        assert!(matches!(
            parse_command("list favorites").unwrap(),
            Command::List {
                target: ListTarget::Likes
            }
        ));
        assert!(matches!(
            parse_command("list dm").unwrap(),
            Command::List {
                target: ListTarget::Dms
            }
        ));
        assert!(matches!(
            parse_command("list convos").unwrap(),
            Command::List {
                target: ListTarget::Conversations
            }
        ));
        assert!(matches!(
            parse_command("list blocked").unwrap(),
            Command::List {
                target: ListTarget::Blocks
            }
        ));
        assert!(matches!(
            parse_command("list muted").unwrap(),
            Command::List {
                target: ListTarget::Mutes
            }
        ));
    }

    #[test]
    fn test_parse_list_invalid_target() {
        let result = parse_command("list invalid_target");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_refine_command() {
        let cmd = parse_command("refine keyword").unwrap();
        assert!(matches!(cmd, Command::Refine { filter } if filter == "keyword"));
    }

    #[test]
    fn test_parse_refine_alias() {
        let cmd = parse_command("r multiple words here").unwrap();
        assert!(matches!(cmd, Command::Refine { filter } if filter == "multiple words here"));
    }

    #[test]
    fn test_parse_refine_empty_fails() {
        let result = parse_command("refine");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_more_command() {
        let cmd = parse_command("more").unwrap();
        assert!(matches!(cmd, Command::More));
    }

    #[test]
    fn test_parse_more_alias() {
        let cmd = parse_command("m").unwrap();
        assert!(matches!(cmd, Command::More));
    }

    #[test]
    fn test_parse_show_command() {
        let cmd = parse_command("show 5").unwrap();
        assert!(matches!(cmd, Command::Show { index: 5 }));
    }

    #[test]
    fn test_parse_show_no_number_fails() {
        let result = parse_command("show");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_show_invalid_number_fails() {
        let result = parse_command("show abc");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_export_default_format() {
        let cmd = parse_command("export").unwrap();
        assert!(matches!(
            cmd,
            Command::Export {
                format: ExportFormat::Json
            }
        ));
    }

    #[test]
    fn test_parse_export_csv() {
        let cmd = parse_command("export csv").unwrap();
        assert!(matches!(
            cmd,
            Command::Export {
                format: ExportFormat::Csv
            }
        ));
    }

    #[test]
    fn test_parse_export_alias() {
        let cmd = parse_command("e json").unwrap();
        assert!(matches!(
            cmd,
            Command::Export {
                format: ExportFormat::Json
            }
        ));
    }

    #[test]
    fn test_parse_export_invalid_format() {
        let result = parse_command("export xml");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_stats_command() {
        let cmd = parse_command("stats").unwrap();
        assert!(matches!(cmd, Command::Stats));
    }

    #[test]
    fn test_parse_help_command() {
        let cmd = parse_command("help").unwrap();
        assert!(matches!(cmd, Command::Help { command: None }));
    }

    #[test]
    fn test_parse_help_with_topic() {
        let cmd = parse_command("help search").unwrap();
        assert!(matches!(cmd, Command::Help { command: Some(ref c) } if c == "search"));
    }

    #[test]
    fn test_parse_help_aliases() {
        assert!(matches!(
            parse_command("h").unwrap(),
            Command::Help { command: None }
        ));
        assert!(matches!(
            parse_command("?").unwrap(),
            Command::Help { command: None }
        ));
    }

    #[test]
    fn test_parse_quit_command() {
        let cmd = parse_command("quit").unwrap();
        assert!(matches!(cmd, Command::Quit));
    }

    #[test]
    fn test_parse_quit_aliases() {
        assert!(matches!(parse_command("exit").unwrap(), Command::Quit));
        assert!(matches!(parse_command("q").unwrap(), Command::Quit));
    }

    #[test]
    fn test_parse_unknown_command() {
        let result = parse_command("unknown_command");
        assert!(result.is_err());
    }

    // ======================== Prompt Formatting Tests ========================

    #[test]
    fn test_prompt_context_normal() {
        let ctx = PromptContext::Normal;
        // We can't easily test ReplSession::format_prompt directly,
        // but we can verify the enum variants exist and are used correctly
        assert!(matches!(ctx, PromptContext::Normal));
    }

    #[test]
    fn test_prompt_context_with_results() {
        let ctx = PromptContext::WithResults(42);
        assert!(matches!(ctx, PromptContext::WithResults(42)));
    }

    #[test]
    fn test_prompt_context_in_conversation() {
        let ctx = PromptContext::InConversation("abc123".to_string());
        assert!(matches!(ctx, PromptContext::InConversation(_)));
    }

    // ======================== Helper Function Tests ========================

    #[test]
    fn test_truncate_text_short() {
        let result = truncate_text("short text", 80);
        assert_eq!(result, "short text");
    }

    #[test]
    fn test_truncate_text_long() {
        let long_text = "a".repeat(100);
        let result = truncate_text(&long_text, 50);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 50);
    }

    #[test]
    fn test_truncate_text_removes_newlines() {
        let result = truncate_text("line1\nline2\rline3", 80);
        assert!(!result.contains('\n'));
        assert!(!result.contains('\r'));
    }

    #[test]
    fn test_truncate_text_exact_length() {
        let text = "exactly ten";
        let result = truncate_text(text, 11);
        assert_eq!(result, text);
    }
}
