# xf (x_find)

**Ultra-fast CLI for searching and querying your Twitter/X data archive.**

Ever wanted to instantly search through years of your tweets, likes, and DMs? `xf` indexes your Twitter data export and provides blazingly fast full-text search with sub-millisecond query latency.

## Features

- **Instant Search**: Sub-millisecond query latency via Tantivy (Lucene-like search engine)
- **Full-Text Search**: BM25 ranking with phrase queries, wildcards, and boolean operators
- **Search Everything**: Tweets, likes, DMs, and Grok conversations
- **Rich CLI**: Colorized output, progress bars, multiple output formats
- **SQLite Storage**: Metadata queries and statistics
- **Privacy-First**: All data stays local on your machine

## Installation

### One-liner (recommended)

```bash
curl -fsSL "https://raw.githubusercontent.com/Dicklesworthstone/x_find/master/install.sh?$(date +%s)" | bash
```

### With options

```bash
# Easy mode: auto-update PATH
curl -fsSL "https://raw.githubusercontent.com/Dicklesworthstone/x_find/master/install.sh?$(date +%s)" | bash -s -- --easy-mode

# Build from source
curl -fsSL "https://raw.githubusercontent.com/Dicklesworthstone/x_find/master/install.sh?$(date +%s)" | bash -s -- --from-source

# System-wide install (requires sudo)
curl -fsSL "https://raw.githubusercontent.com/Dicklesworthstone/x_find/master/install.sh?$(date +%s)" | sudo bash -s -- --system
```

### From source (manual)

```bash
git clone https://github.com/Dicklesworthstone/x_find.git
cd x_find
cargo build --release
cp target/release/xf ~/.local/bin/
```

## Quick Start

### 1. Download your Twitter data

1. Go to [twitter.com/settings/download_your_data](https://twitter.com/settings/download_your_data)
2. Request your archive
3. Wait for the email (can take 24-48 hours)
4. Download and extract the archive

### 2. Index your archive

```bash
xf index /path/to/your-twitter-archive
```

This parses all your data and builds a searchable index. On a typical archive, this takes 10-30 seconds.

### 3. Search!

```bash
# Basic search
xf search "machine learning"

# Search only tweets
xf search "python" --types tweet

# Search DMs
xf search "meeting" --types dm

# Search likes
xf search "interesting article" --types like

# JSON output
xf search "rust" --format json

# Limit results
xf search "AI" --limit 5
```

## Commands

### `xf index <archive_path>`

Index a Twitter data archive.

```bash
xf index ~/Downloads/twitter-archive

# Force re-index (clear existing data)
xf index ~/Downloads/twitter-archive --force

# Index only specific data types
xf index ~/Downloads/twitter-archive --only tweet,like

# Skip certain data types
xf index ~/Downloads/twitter-archive --skip dm,grok
```

### `xf search <query>`

Search the indexed archive.

```bash
# Basic search
xf search "your query"

# Filter by type
xf search "query" --types tweet,dm

# Pagination
xf search "query" --limit 20 --offset 40

# Output formats
xf search "query" --format json
xf search "query" --format csv
xf search "query" --format compact
```

**Query syntax:**
- Simple terms: `machine learning`
- Phrases: `"exact phrase"`
- Boolean: `rust AND async`
- Exclusion: `python NOT snake`

### `xf stats`

Show archive statistics.

```bash
xf stats

# JSON output
xf stats --format json

# Detailed breakdown
xf stats --detailed
```

### `xf tweet <id>`

Show details for a specific tweet.

```bash
xf tweet 1234567890

# Show engagement metrics
xf tweet 1234567890 --engagement
```

### `xf config`

Manage configuration.

```bash
# Show current config
xf config --show
```

## Output Formats

| Format | Description |
|--------|-------------|
| `text` | Human-readable with colors (default) |
| `json` | Compact JSON |
| `json-pretty` | Pretty-printed JSON |
| `csv` | Comma-separated values |
| `compact` | One result per line |

## Data Types

| Type | Description |
|------|-------------|
| `tweet` | Your tweets |
| `like` | Tweets you've liked |
| `dm` | Direct messages |
| `grok` | Grok AI conversations |
| `follower` | Your followers |
| `following` | Accounts you follow |
| `block` | Blocked accounts |
| `mute` | Muted accounts |

## Storage Locations

By default, `xf` stores data in:

| Platform | Location |
|----------|----------|
| macOS | `~/Library/Application Support/xf/` |
| Linux | `~/.local/share/xf/` |
| Windows | `%LOCALAPPDATA%\xf\` |

Override with environment variables:
- `XF_DB`: Path to SQLite database
- `XF_INDEX`: Path to search index directory

## Performance

`xf` is designed for speed:

- **Indexing**: ~10,000 tweets/second
- **Search**: Sub-millisecond for most queries
- **Memory**: Efficient memory-mapped index files
- **Parallelism**: Multi-threaded parsing and indexing

### Benchmarks

On a typical archive (12,000 tweets, 40,000 likes):
- Index time: ~5 seconds
- Search latency: <1ms
- Database size: ~10MB
- Index size: ~15MB

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                     Twitter Archive                      │
│   (tweets.js, like.js, direct-messages.js, etc.)        │
└─────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                    Parser (parser.rs)                    │
│   Handles window.YTD.* JavaScript format                │
└─────────────────────────────────────────────────────────┘
                            │
              ┌─────────────┴─────────────┐
              ▼                           ▼
┌──────────────────────┐    ┌──────────────────────────┐
│  SQLite (storage.rs) │    │  Tantivy (search.rs)     │
│  - Metadata queries  │    │  - Full-text search      │
│  - Statistics        │    │  - BM25 ranking          │
│  - FTS5 fallback     │    │  - Phrase queries        │
└──────────────────────┘    └──────────────────────────┘
              │                           │
              └─────────────┬─────────────┘
                            ▼
┌─────────────────────────────────────────────────────────┐
│                      CLI (cli.rs)                        │
│   clap-based command parsing with rich output           │
└─────────────────────────────────────────────────────────┘
```

## Building from Source

Requirements:
- Rust nightly (automatically selected via `rust-toolchain.toml`)
- Git

```bash
git clone https://github.com/Dicklesworthstone/x_find.git
cd x_find
cargo build --release
```

### Running Tests

```bash
cargo test
```

### Running Benchmarks

```bash
cargo bench
```

## Contributing

This project is maintained by Jeffrey Emanuel. While contributions are welcome, please note that the codebase follows specific patterns and conventions. Feel free to open issues for bugs or feature requests.

## License

MIT License - see [LICENSE](LICENSE) for details.

## FAQ

### Why "xf"?

`xf` stands for "x_find" - a fast way to find things in your X (formerly Twitter) data.

### Is my data safe?

Yes! All data stays on your local machine. `xf` never sends data anywhere. The search index and database are stored locally.

### Can I search old tweets?

Yes, if they're in your archive. Twitter includes all your tweets in the data export.

### What about deleted tweets?

Twitter includes recently deleted tweets (last 14 days) in a separate file. `xf` can index these too.

### How do I update?

```bash
curl -fsSL "https://raw.githubusercontent.com/Dicklesworthstone/x_find/master/install.sh?$(date +%s)" | bash
```

### The search is slow. What's wrong?

First search after restart may be slower as the index loads. Subsequent searches should be sub-millisecond. If consistently slow, try rebuilding the index with `xf index --force`.

---

Built with Rust, Tantivy, and SQLite. Inspired by the need to actually search through years of tweets.
