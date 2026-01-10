# Changelog

All notable changes to xf will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Custom error types (`XfError`) with rich context and suggestions
- Performance budget system with timing utilities
- Comprehensive unit tests for parser, search, and storage modules
- CI/CD pipeline with GitHub Actions
  - Format checking
  - Clippy lints
  - Cross-platform tests (Linux, macOS, Windows)
  - Code coverage reporting
  - Security audits
- Automated release workflow
  - Multi-platform binary builds
  - SHA256 checksums
  - GitHub Release creation
- Layered configuration system
  - User config file (`~/.config/xf/config.toml`)
  - Environment variable overrides (`XF_*`)
  - CLI argument precedence
- Documentation
  - Architecture guide
  - Performance guide
  - Troubleshooting guide
  - Development guide
  - Contributing guidelines
- Real benchmarks with Criterion
  - Search benchmarks (simple, phrase, boolean, prefix)
  - Indexing benchmarks
  - Storage benchmarks
  - Scalability benchmarks

### Changed
- Project renamed from `x_find` to `xf`
- Updated branding from Twitter/X to X throughout codebase

### Fixed
- UTF-8 panic in truncate function when string ends with multi-byte character
- Unused imports and variables
- Documentation comments

## [0.1.0] - 2025-01-08

### Added
- Initial release
- Tantivy-based full-text search engine
  - BM25 ranking
  - Phrase queries
  - Boolean queries (AND, OR, NOT)
  - Prefix matching with edge n-grams
- SQLite storage with FTS5 fallback
- Support for X archive data types:
  - Tweets
  - Likes
  - Direct Messages
  - Grok conversations
  - Followers/Following
  - Blocks/Mutes
- CLI commands:
  - `index` - Index an X archive
  - `search` - Search indexed data
  - `stats` - Show archive statistics
- Output formats:
  - Text (default)
  - JSON
  - JSON (pretty)
  - Compact
  - CSV
- Parallel parsing with rayon
- Progress indicators for indexing

### Technical Details
- Rust Edition 2024
- Requires nightly toolchain
- Key dependencies:
  - tantivy 0.22
  - rusqlite 0.32
  - clap 4.5
  - chrono 0.4
  - rayon 1.10

---

## Release Notes Format

Each release should include:

1. **Added** - New features
2. **Changed** - Changes in existing functionality
3. **Deprecated** - Features that will be removed
4. **Removed** - Removed features
5. **Fixed** - Bug fixes
6. **Security** - Security-related changes

[Unreleased]: https://github.com/Dicklesworthstone/xf/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Dicklesworthstone/xf/releases/tag/v0.1.0
