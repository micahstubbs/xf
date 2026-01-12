# xf

<div align="center">
  <img src="xf_illustration.webp" alt="xf - Ultra-fast CLI for searching your X data archive">
</div>

<div align="center">

[![CI](https://github.com/Dicklesworthstone/xf/actions/workflows/ci.yml/badge.svg)](https://github.com/Dicklesworthstone/xf/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

</div>

Ultra-fast CLI for searching and querying your X data archive with sub-millisecond latency.

<div align="center">
<h3>Quick Install</h3>

```bash
curl -fsSL https://raw.githubusercontent.com/Dicklesworthstone/xf/main/install.sh | bash
```

<p><em>Works on Linux and macOS. Auto-detects your platform and downloads the right binary.</em></p>
</div>

---

## TL;DR

**The Problem**: X lets you download all your data, but actually *finding* anything in that archive is painful. The built-in HTML viewer is slow and clunky, there's no real search, and your data is scattered across separate files.

**The Solution**: `xf` indexes your X (formerly Twitter) data export and provides blazingly fast full-text search across tweets, likes, DMs, and Grok conversations—all from the command line.

### Why Use xf?

| Feature | What It Does |
|---------|--------------|
| **Sub-Millisecond Search** | Tantivy-powered full-text search with BM25 ranking |
| **Search Everything** | Tweets, likes, DMs, and Grok conversations in one place |
| **Rich Query Syntax** | Phrases, wildcards, boolean operators (`AND`, `OR`, `NOT`) |
| **DM Context** | View full conversation threads with search matches highlighted |
| **Multiple Formats** | JSON, CSV, compact, or colorized terminal output |
| **Privacy-First** | All data stays local on your machine—nothing sent anywhere |
| **Fast Indexing** | ~10,000 documents/second with parallel parsing |

### Quick Example

```bash
# Index your archive (one-time setup, ~5 seconds)
$ xf index ~/x-archive

# Search across everything
$ xf search "machine learning"

# Search only your DMs with full conversation context
$ xf search "meeting tomorrow" --types dm --context

# Export results as JSON
$ xf search "rust async" --format json --limit 50
```
---
## Prepared Blurb for AGENTS.md Files:
```
## xf — X Archive Search

Ultra-fast local search for X (Twitter) data archives. Parses `window.YTD.*` JavaScript format from X data exports. Sub-millisecond full-text search via Tantivy + SQLite storage.

### Core Workflow

```bash
# 1. Index archive (one-time, ~5-30 seconds)
xf index ~/x-archive
xf index ~/x-archive --force          # Rebuild from scratch
xf index ~/x-archive --only tweet,dm  # Index specific types
xf index ~/x-archive --skip grok      # Skip specific types

# 2. Search
xf search "machine learning"          # Search all indexed content
xf search "meeting" --types dm        # DMs only
xf search "rust async" --types tweet  # Tweets only
xf search "article" --types like      # Liked tweets only
xf search "claude" --types grok       # Grok conversations only

Search Syntax

xf search "exact phrase"              # Phrase match (quotes matter)
xf search "rust AND async"            # Boolean AND
xf search "python OR javascript"      # Boolean OR
xf search "python NOT snake"          # Exclusion
xf search "rust*"                     # Wildcard prefix

Key Flags

--format json                         # Machine-readable output (use this!)
--format csv                          # Spreadsheet export
--limit 50                            # Results count (default: 20)
--offset 20                           # Pagination
--context                             # Full DM conversation thread (--types dm only)
--since "2024-01-01"                  # Date filter (supports natural language)
--until "last week"                   # Date filter
--sort date|date_desc|relevance|engagement

Other Commands

xf stats                              # Archive overview (counts, date range)
xf stats --detailed                   # Full analytics (temporal, engagement, content)
xf stats --format json                # Machine-readable stats
xf tweet <id>                         # Show specific tweet by ID
xf tweet <id> --engagement            # Include engagement metrics
xf list tweets --limit 20             # Browse indexed tweets
xf list dms                           # Browse DM conversations
xf doctor                             # Health checks (archive, DB, index)
xf shell                              # Interactive REPL

Data Types

tweet (your posts), like (liked tweets), dm (direct messages), grok (AI chats), follower, following, block, mute

Storage

- Database: ~/.local/share/xf/xf.db (override: XF_DB env)
- Index: ~/.local/share/xf/xf_index/ (override: XF_INDEX env)
- Archive format: Expects data/ directory with tweets.js, like.js, direct-messages.js, etc.

Notes

- First search after restart may be slower (index loading). Subsequent searches <1ms.
- --context only works with --types dm — shows full conversation around matches.
- All data stays local. No network access.
```

---

## Origins & Authors

This project was created by Jeffrey Emanuel after realizing that X's data export, while comprehensive, lacks any useful search functionality.

- **[Jeffrey Emanuel](https://github.com/Dicklesworthstone)** - Creator and maintainer

## Getting Your X Data Archive

Before using `xf`, you need to download your data from X. Here's the complete process:

### Step 1: Request Your Archive

1. **Log into X** at [x.com](https://x.com) or [twitter.com](https://twitter.com)
2. **Navigate to Settings**:
   - Click "More" (...) in the left sidebar
   - Select "Settings and Support" -> "Settings and privacy"
   - Or go directly to: [x.com/settings/download_your_data](https://x.com/settings/download_your_data)
3. **Request your archive**:
   - Under "Download an archive of your data", click "Request archive"
   - You may need to verify your identity (password, 2FA)
   - Select what data you want (recommend "All data" for complete archive)

### Step 2: Wait for Processing

X needs time to compile your archive:
- **Typical wait time**: 24-48 hours (can be longer for large accounts)
- You'll receive an **email notification** when it's ready
- You can also check the same settings page for status updates
- The link expires after a few days, so download promptly!

### Step 3: Download and Extract

1. **Download**: Click the link in your email or on the settings page
   - File will be named something like `twitter-2026-01-09-abc123.zip`
   - Size varies: typically 50MB to several GB depending on your activity and media
2. **Extract**: Unzip the archive to a folder
   ```bash
   unzip twitter-2026-01-09-abc123.zip -d ~/x-archive
   ```

### What's Inside the Archive

Your extracted archive contains:

```
x-archive/
├── Your archive.html      # Browser viewer (open this to explore manually)
├── data/
│   ├── tweets.js          # All your tweets
│   ├── like.js            # Tweets you've liked
│   ├── direct-messages.js # DM conversations
│   ├── follower.js        # Your followers
│   ├── following.js       # Accounts you follow
│   ├── grok-conversation-...  # Grok AI chats (if any)
│   ├── account.js         # Account info
│   ├── profile.js         # Profile data
│   └── ...                # Many other data files
└── assets/
    └── images/            # Media files (can be large!)
```

The data files use a JavaScript format like:
```javascript
window.YTD.tweets.part0 = [
  { "tweet": { "id": "123...", "full_text": "Hello world!", ... } },
  ...
]
```

`xf` knows how to parse this format and extract all your content.

## Installation

### Quick Install (Recommended)

The easiest way to install is using the install script, which downloads a prebuilt binary for your platform:

```bash
curl -fsSL https://raw.githubusercontent.com/Dicklesworthstone/xf/main/install.sh | bash
```

**With options:**

```bash
# Easy mode: auto-update PATH in shell rc files
curl -fsSL https://raw.githubusercontent.com/Dicklesworthstone/xf/main/install.sh | bash -s -- --easy-mode

# Install specific version
curl -fsSL https://raw.githubusercontent.com/Dicklesworthstone/xf/main/install.sh | bash -s -- --version v0.1.0

# Install to /usr/local/bin (system-wide, requires sudo)
curl -fsSL https://raw.githubusercontent.com/Dicklesworthstone/xf/main/install.sh | sudo bash -s -- --system

# Build from source instead of downloading binary
curl -fsSL https://raw.githubusercontent.com/Dicklesworthstone/xf/main/install.sh | bash -s -- --from-source
```

> **Note:** If you have [gum](https://github.com/charmbracelet/gum) installed, the installer will use it for fancy terminal formatting.

The install script:
- Automatically detects your OS and architecture
- Downloads the appropriate prebuilt binary
- Verifies SHA256 checksums for security
- Falls back to building from source if no prebuilt is available
- Offers to update your PATH

### From Source (requires Rust nightly)

This project uses Rust Edition 2024 features and requires the nightly toolchain. The repository includes a `rust-toolchain.toml` that automatically selects the correct toolchain.

```bash
# Install Rust nightly if you don't have it
rustup install nightly

# Install directly from GitHub
cargo +nightly install --git https://github.com/Dicklesworthstone/xf.git
```

### Manual Build

```bash
git clone https://github.com/Dicklesworthstone/xf.git
cd xf
# rust-toolchain.toml automatically selects nightly
cargo build --release
cp target/release/xf ~/.local/bin/
```

### Prebuilt Binaries

Prebuilt binaries are available for:
- Linux x86_64 (`x86_64-unknown-linux-gnu`)
- Linux ARM64 (`aarch64-unknown-linux-gnu`)
- macOS Intel (`x86_64-apple-darwin`)
- macOS Apple Silicon (`aarch64-apple-darwin`)

Download from [GitHub Releases](https://github.com/Dicklesworthstone/xf/releases) and verify the SHA256 checksum.

## Quick Start

### 1. Index your archive

```bash
xf index ~/x-archive
```

This parses all your data and builds a searchable index. On a typical archive, this takes 10-30 seconds.

### 2. Search!

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

Index an X data archive.

```bash
xf index ~/Downloads/x-archive

# Force re-index (clear existing data)
xf index ~/Downloads/x-archive --force

# Index only specific data types
xf index ~/Downloads/x-archive --only tweet,like

# Skip certain data types
xf index ~/Downloads/x-archive --skip dm,grok
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

# DM context: show full conversation with matches highlighted
xf search "meeting" --types dm --context
xf search "meeting" --types dm --context --format json
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

### `xf update`

Check for updates.

```bash
xf update
```

### `xf completions <shell>`

Generate shell completions.

```bash
# Bash
xf completions bash > ~/.local/share/bash-completion/completions/xf

# Zsh
xf completions zsh > ~/.zfunc/_xf

# Fish
xf completions fish > ~/.config/fish/completions/xf.fish
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

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                       X Data Archive                             │
│   (tweets.js, like.js, direct-messages.js, etc.)                │
└─────────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Parser (parser.rs)                            │
│   Handles window.YTD.* JavaScript format with rayon parallelism │
└─────────────────────────────────────────────────────────────────┘
                            │
              ┌─────────────┴─────────────┐
              ▼                           ▼
┌──────────────────────┐    ┌──────────────────────────┐
│  SQLite (storage.rs) │    │  Tantivy (search.rs)     │
│  - Metadata storage  │    │  - Full-text search      │
│  - Statistics        │    │  - BM25 ranking          │
│  - FTS5 for fallback │    │  - Phrase queries        │
│  - Tweet lookup      │    │  - Boolean operators     │
└──────────────────────┘    └──────────────────────────┘
              │                           │
              └─────────────┬─────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                      CLI (cli.rs)                                │
│   clap-based command parsing with rich colored output           │
└─────────────────────────────────────────────────────────────────┘
```

### Processing Pipeline

**Stage 1: Archive Parsing**
- Reads JavaScript files from the archive's `data/` directory
- Strips `window.YTD.<type>.part0 = ` prefix to extract JSON
- Uses `rayon` for parallel parsing of large files

**Stage 2: Storage**
- Normalizes data into structured models (`Tweet`, `Like`, `DirectMessage`, etc.)
- Stores in SQLite with FTS5 virtual tables for fallback search
- Maintains statistics and metadata

**Stage 3: Indexing**
- Feeds content to Tantivy search engine
- Creates inverted index with BM25 scoring
- Supports prefix queries via edge n-grams

**Stage 4: Search**
- Parses user query with Tantivy's query parser
- Returns ranked results with scores and highlights
- Joins with SQLite for full metadata retrieval

## Performance

`xf` is designed for speed:

- **Indexing**: ~10,000 documents/second
- **Search**: Sub-millisecond for most queries
- **Memory**: Efficient memory-mapped index files
- **Parallelism**: Multi-threaded parsing via rayon

### Benchmarks

On a typical archive (12,000 tweets, 40,000 likes):
- Index time: ~5 seconds
- Search latency: <1ms
- Database size: ~10MB
- Index size: ~15MB

### Performance Optimizations

**1. Lazy Static Initialization**
- Regex patterns and search readers are compiled once on first use
- Subsequent operations reuse compiled resources

**2. Parallel Parsing**
- Uses `rayon` to parse archive files in parallel
- Takes full advantage of multi-core CPUs

**3. Memory-Mapped Index**
- Tantivy uses memory-mapped files for the search index
- OS manages caching automatically

**4. Release Profile**

```toml
[profile.release]
opt-level = "z"     # Optimize for size (lean binary)
lto = true          # Link-time optimization across crates
codegen-units = 1   # Single codegen unit for better optimization
panic = "abort"     # Smaller binary, no unwinding overhead
strip = true        # Remove debug symbols
```

## Building from Source

Requirements:
- Rust nightly (automatically selected via `rust-toolchain.toml`)
- Git

```bash
git clone https://github.com/Dicklesworthstone/xf.git
cd xf
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

## FAQ

### Why "xf"?

`xf` stands for "x_find" - a fast way to find things in your X (formerly Twitter) data.

### Is my data safe?

Yes! All data stays on your local machine. `xf` never sends data anywhere. The search index and database are stored locally.

### Can I search old tweets?

Yes, if they're in your archive. X includes all your tweets in the data export.

### What about deleted tweets?

X includes recently deleted tweets (within the last 30 days) in a separate file. `xf` can index these too.

### How do I update?

```bash
curl -fsSL https://raw.githubusercontent.com/Dicklesworthstone/xf/main/install.sh | bash
```

Or use the built-in command:

```bash
xf update
```

### The search is slow. What's wrong?

First search after restart may be slower as the index loads. Subsequent searches should be sub-millisecond. If consistently slow, try rebuilding the index with `xf index --force`.

### Can I search multiple archives?

Currently, `xf` supports one archive at a time. To switch archives, re-run `xf index` with the new path (use `--force` to clear the old data).

### What query syntax is supported?

Tantivy's query parser supports:
- Terms: `word`
- Phrases: `"multiple words"`
- Boolean: `term1 AND term2`, `term1 OR term2`
- Exclusion: `term1 NOT term2`
- Wildcards: `rust*`
- Field-specific: `type:tweet text:rust`

## Contributing

*About Contributions:* Please don't take this the wrong way, but I do not accept outside contributions for any of my projects. I simply don't have the mental bandwidth to review anything, and it's my name on the thing, so I'm responsible for any problems it causes; thus, the risk-reward is highly asymmetric from my perspective. I'd also have to worry about other "stakeholders," which seems unwise for tools I mostly make for myself for free. Feel free to submit issues, and even PRs if you want to illustrate a proposed fix, but know I won't merge them directly. Instead, I'll have Claude or Codex review submissions via `gh` and independently decide whether and how to address them. Bug reports in particular are welcome. Sorry if this offends, but I want to avoid wasted time and hurt feelings. I understand this isn't in sync with the prevailing open-source ethos that seeks community contributions, but it's the only way I can move at this velocity and keep my sanity.

## License

MIT - see [LICENSE](LICENSE) for details.

---

Built with Rust, Tantivy, and SQLite. Inspired by the need to actually search through years of tweets.
