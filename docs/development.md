# xf Development Guide

This guide covers setting up a development environment and contributing to xf.

## Prerequisites

### Required

- **Rust nightly**: xf uses Edition 2024 features
- **Git**: For version control
- **SQLite3**: For database inspection (usually pre-installed)

### Installation

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install nightly toolchain
rustup install nightly
rustup default nightly

# Verify installation
rustc --version  # Should show nightly-YYYY-MM-DD
cargo --version
```

## Getting Started

### Clone and Build

```bash
# Clone repository
git clone https://github.com/Dicklesworthstone/xf.git
cd xf

# Build in debug mode (faster compilation)
cargo build

# Build in release mode (optimized)
cargo build --release

# Run tests
cargo test

# Run with example
cargo run -- --help
```

### Project Structure

```
xf/
├── src/
│   ├── main.rs        # CLI entry point
│   ├── lib.rs         # Library root, public exports
│   ├── cli.rs         # Command-line interface definitions
│   ├── config.rs      # Configuration management
│   ├── error.rs       # Custom error types
│   ├── model.rs       # Data models (Tweet, Like, DM, etc.)
│   ├── parser.rs      # Archive parsing logic
│   ├── perf.rs        # Performance budgets
│   ├── search.rs      # Tantivy search engine
│   └── storage.rs     # SQLite storage layer
├── benches/
│   └── search_perf.rs # Criterion benchmarks
├── tests/
│   └── integration/   # Integration tests
├── docs/
│   ├── architecture.md
│   ├── performance.md
│   ├── troubleshooting.md
│   └── development.md # This file
└── .github/
    └── workflows/     # CI/CD pipelines
```

## Development Workflow

### 1. Create a Branch

```bash
git checkout -b feature/my-feature
# or
git checkout -b fix/issue-123
```

### 2. Make Changes

Follow these guidelines:

- Run `cargo fmt` before committing
- Run `cargo clippy` to check for lints
- Add tests for new functionality
- Update documentation if needed

### 3. Test Changes

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture

# Run benchmarks
cargo bench
```

### 4. Submit PR

```bash
# Push changes
git push -u origin feature/my-feature

# Create PR on GitHub
```

## Code Style

### Formatting

xf uses standard Rust formatting with `rustfmt`:

```bash
# Format all code
cargo fmt

# Check formatting without modifying
cargo fmt -- --check
```

### Linting

```bash
# Run Clippy with all warnings as errors
cargo clippy --all-targets --all-features -- -D warnings
```

### Documentation

All public items should have documentation:

```rust
/// Brief description of the function.
///
/// # Arguments
///
/// * `param` - Description of parameter
///
/// # Returns
///
/// Description of return value
///
/// # Errors
///
/// Describes when this function returns an error
///
/// # Examples
///
/// ```rust
/// let result = my_function(arg);
/// assert!(result.is_ok());
/// ```
pub fn my_function(param: &str) -> Result<Output> {
    // ...
}
```

## Testing

### Unit Tests

Located in the same file as the code:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        let result = function_under_test();
        assert_eq!(result, expected);
    }
}
```

### Integration Tests

Located in `tests/`:

```rust
// tests/integration/search_test.rs
use xf::{SearchEngine, Storage};

#[test]
fn test_end_to_end_search() {
    // Setup
    let storage = Storage::new_in_memory().unwrap();
    let search = SearchEngine::new_in_memory().unwrap();

    // Test
    // ...
}
```

### Test Data

For testing, use the test fixtures in `tests/fixtures/` or create minimal test data:

```rust
fn create_test_tweet() -> Tweet {
    Tweet {
        id: "123".to_string(),
        full_text: "Test tweet content".to_string(),
        created_at: Utc::now(),
        ..Default::default()
    }
}
```

## Benchmarking

### Running Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench --bench search_perf -- search_simple

# Save baseline for comparison
cargo bench -- --save-baseline main

# Compare to baseline
cargo bench -- --baseline main
```

### Writing Benchmarks

```rust
use criterion::{criterion_group, criterion_main, Criterion};

fn my_benchmark(c: &mut Criterion) {
    // Setup (not measured)
    let data = setup_test_data();

    c.bench_function("operation_name", |b| {
        b.iter(|| {
            // Code to benchmark
            operation(&data)
        })
    });
}

criterion_group!(benches, my_benchmark);
criterion_main!(benches);
```

## Debugging

### Logging

Enable debug logging:

```bash
RUST_LOG=xf=debug cargo run -- search "query"
```

### Using a Debugger

```bash
# With LLDB (macOS)
rust-lldb target/debug/xf -- search "query"

# With GDB (Linux)
rust-gdb target/debug/xf -- search "query"
```

### Profiling

```bash
# CPU profiling with flamegraph
cargo install flamegraph
cargo flamegraph -- search "query"

# Memory profiling
cargo install cargo-instruments  # macOS only
cargo instruments -t "Allocations" -- search "query"
```

## Architecture Notes

### Error Handling

Use the custom `XfError` type from `src/error.rs`:

```rust
use crate::error::{Result, XfError};

fn my_function() -> Result<Output> {
    // Use ? for propagation
    let data = read_data()?;

    // Create specific errors
    if data.is_empty() {
        return Err(XfError::ParseError {
            file: "data.json".to_string(),
            message: "Empty data".to_string(),
            line: None,
        });
    }

    Ok(process(data))
}
```

### Performance

Check operations against performance budgets:

```rust
use crate::perf::{self, Timer};

fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
    let _timer = Timer::new(perf::SEARCH_SIMPLE);

    // Operation is automatically timed
    // Warnings logged if exceeds budget
    self.execute_search(query)
}
```

### Configuration

Access configuration through the `Config` struct:

```rust
use crate::config::Config;

fn setup() -> Result<()> {
    let config = Config::load();

    let db_path = config.db_path();
    let index_path = config.index_path();

    // Use configuration values
    Ok(())
}
```

## Release Process

### Version Bumping

1. Update `Cargo.toml` version
2. Update `CHANGELOG.md`
3. Commit changes
4. Create and push tag:

```bash
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```

### Automated Release

The GitHub Actions workflow automatically:
1. Builds binaries for all platforms
2. Creates GitHub release
3. Uploads binaries and checksums

## Common Tasks

### Adding a New Command

1. Add variant to `Commands` enum in `src/cli.rs`
2. Implement handler in `src/main.rs`
3. Add tests
4. Update documentation

### Adding a New Data Type

1. Define struct in `src/model.rs`
2. Add parsing logic in `src/parser.rs`
3. Add storage table in `src/storage.rs`
4. Add search indexing in `src/search.rs`
5. Add tests for each layer

### Modifying the Schema

1. Update schema in `src/storage.rs`
2. Add migration logic if needed
3. Update model structs
4. Update search schema
5. Test with `cargo test`

## Dependencies

Key dependencies and their purposes:

| Crate | Purpose |
|-------|---------|
| `tantivy` | Full-text search engine |
| `rusqlite` | SQLite database |
| `clap` | CLI argument parsing |
| `serde` | Serialization |
| `chrono` | Date/time handling |
| `rayon` | Parallel processing |
| `thiserror` | Error definitions |
| `tracing` | Logging/diagnostics |

## Getting Help

- Check existing issues on GitHub
- Join discussions
- Read the architecture documentation
- Ask questions in issues
