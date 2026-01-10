# xf Architecture

This document describes the internal architecture of xf, the ultra-fast X data archive search tool.

## System Overview

xf uses a hybrid storage strategy combining a full-text search engine with a relational database:

```
┌─────────────────────────────────────────────────────────────────┐
│                        X Data Archive                            │
│                    (ZIP or extracted folder)                     │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Archive Parser                              │
│                       (parser.rs)                                │
│  • Parses JavaScript-wrapped JSON files                         │
│  • Normalizes data into Rust structs                            │
│  • Parallel processing with rayon                               │
└─────────────────────────────┬───────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼                               ▼
┌─────────────────────────┐     ┌─────────────────────────┐
│    Tantivy Search       │     │    SQLite Storage       │
│     (search.rs)         │     │     (storage.rs)        │
├─────────────────────────┤     ├─────────────────────────┤
│ • Full-text indexing    │     │ • Metadata storage      │
│ • BM25 ranking          │     │ • FTS5 fallback search  │
│ • Sub-ms queries        │     │ • Statistics queries    │
│ • Prefix matching       │     │ • Structured queries    │
└─────────────────────────┘     └─────────────────────────┘
              │                               │
              └───────────────┬───────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                         CLI Layer                                │
│                        (main.rs)                                 │
│  • Command parsing (clap)                                       │
│  • Output formatting                                            │
│  • Progress indicators                                          │
└─────────────────────────────────────────────────────────────────┘
```

## Data Flow

### Indexing Flow

```
1. User runs: xf index ~/twitter-archive

2. Archive Parser:
   ├── Parse manifest.js → ArchiveInfo
   ├── Parse tweets.js → Vec<Tweet>
   ├── Parse like.js → Vec<Like>
   ├── Parse direct-messages.js → Vec<DmConversation>
   ├── Parse grok-chat-item.js → Vec<GrokMessage>
   └── Parse follower.js, following.js, etc.

3. Storage Layer:
   ├── Create SQLite database
   ├── Store all records with indexes
   └── Build FTS5 virtual tables

4. Search Engine:
   ├── Create Tantivy index
   ├── Index text fields with BM25
   └── Generate prefix n-grams
```

### Search Flow

```
1. User runs: xf search "rust programming"

2. Query Parsing:
   ├── Parse boolean operators (AND, OR, NOT)
   ├── Handle phrase queries ("exact match")
   └── Apply type filters (--types tweet,dm)

3. Tantivy Search:
   ├── Execute query against index
   ├── Rank results by BM25 score
   └── Return top N document IDs

4. Result Enrichment:
   ├── Fetch full records from SQLite
   └── Format output (text, JSON, CSV)
```

## Module Details

### parser.rs - Archive Parser

The parser handles X's unique data export format:

```javascript
// X export format (tweets.js):
window.YTD.tweets.part0 = [
  {"tweet": {"id": "123", "full_text": "Hello world", ...}},
  ...
]
```

Key responsibilities:
- Strip JavaScript wrapper to extract JSON
- Parse date formats (X uses multiple formats)
- Handle missing/optional fields gracefully
- Parallel parsing with rayon for large archives

### search.rs - Tantivy Search Engine

Schema design:

| Field | Type | Purpose |
|-------|------|---------|
| id | STRING, STORED | Unique document identifier |
| text | TEXT, STORED | Main searchable content |
| text_prefix | TEXT | Edge n-grams for prefix matching |
| type | STRING, STORED | Document type (tweet, like, dm, grok) |
| created_at | I64, INDEXED, FAST | Timestamp for sorting/filtering |
| metadata | TEXT, STORED | JSON blob for extra data |

Key features:
- BM25 ranking for relevance
- Phrase queries with position indexing
- Boolean query support (AND, OR, NOT)
- Sub-millisecond query latency

### storage.rs - SQLite Storage

Table structure (simplified):

```sql
-- Main tweets table
CREATE TABLE tweets (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    full_text TEXT NOT NULL,
    source TEXT,
    favorite_count INTEGER,
    retweet_count INTEGER,
    -- ... more fields
);

-- FTS5 virtual table for fallback search
CREATE VIRTUAL TABLE fts_tweets USING fts5(
    tweet_id,
    full_text,
    content='tweets',
    content_rowid='rowid'
);
```

Key features:
- WAL mode for concurrent reads
- Prepared statements for performance
- FTS5 for backup search capability
- Efficient batch inserts

## Design Decisions

### Why Both Tantivy and SQLite?

| Capability | Tantivy | SQLite |
|------------|---------|--------|
| Full-text search | Excellent | Good (FTS5) |
| BM25 ranking | Native | Manual |
| Query latency | <1ms | 5-50ms |
| Structured queries | Limited | Excellent |
| Storage efficiency | Moderate | Excellent |
| Metadata storage | JSON blob | Native tables |

The hybrid approach gives us:
- **Tantivy** for blazing-fast text search
- **SQLite** for structured queries and statistics

### Why Parse JavaScript Files?

X exports data as JavaScript files (`window.YTD.* = [...]`) rather than
pure JSON. This is likely for browser-based viewing in their archive
viewer. We strip the JS wrapper and parse the underlying JSON.

### Why Parallel Parsing?

Large archives can have hundreds of MB of data. Parallel parsing with
rayon provides 3-4x speedup on typical multi-core systems.

## Performance Characteristics

| Operation | Target | Typical |
|-----------|--------|---------|
| Simple search | <1ms | 0.3ms |
| Phrase search | <5ms | 1-2ms |
| Index 10K tweets | <1s | 500ms |
| Index 100K tweets | <10s | 5s |
| Storage lookup | <1ms | 0.1ms |
| Statistics query | <10ms | 5ms |

## Future Considerations

1. **Incremental Indexing**: Currently we require full re-index for updates
2. **Remote Search**: Could expose search via HTTP API
3. **Multiple Archives**: Support for searching across multiple accounts
4. **Export Formats**: More export options (Markdown, HTML)
