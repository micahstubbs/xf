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
    Stats,
    Help { command: Option<String> },
    Quit,
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
        let results = self.search.search(query, None, 20)?;
        let count = results.len();
        self.last_results = results;
        self.last_query = Some(query.to_string());
        self.prompt_context = PromptContext::WithResults(count);

        println!("{} {}", count.to_string().cyan(), "results".dimmed());
        print_results(&self.last_results);
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

fn print_results(results: &[SearchResult]) {
    for (idx, result) in results.iter().take(10).enumerate() {
        let text = truncate_text(&result.text, 80);
        println!(
            "{:>3}. [{}] {}",
            idx + 1,
            result.result_type.to_string().cyan(),
            text
        );
    }
    if results.len() > 10 {
        println!("{}", "… more results available".dimmed());
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
        Some("search") => {
            println!("search <query>  - search all indexed content");
        }
        Some("stats") => {
            println!("stats           - show archive statistics");
        }
        Some("quit" | "exit") => {
            println!("quit            - exit the REPL");
        }
        _ => {
            println!("{}", "Commands:".bold().cyan());
            println!("  search <query>  - search all indexed content");
            println!("  stats           - show archive statistics");
            println!("  help [command]  - show help");
            println!("  quit            - exit");
        }
    }
}
