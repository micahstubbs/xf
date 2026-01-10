# Contributing to xf

Thank you for your interest in contributing to xf! This document provides guidelines and information about contributing to the project.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Making Changes](#making-changes)
- [Pull Request Process](#pull-request-process)
- [Coding Standards](#coding-standards)
- [Testing](#testing)
- [Documentation](#documentation)

## Code of Conduct

This project follows the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct). Please be respectful and constructive in all interactions.

## Getting Started

### Prerequisites

- Rust nightly toolchain (xf uses Edition 2024 features)
- Git
- A working knowledge of Rust

### Development Setup

```bash
# Clone the repository
git clone https://github.com/Dicklesworthstone/xf.git
cd xf

# Ensure you're using nightly Rust
rustup install nightly
rustup default nightly

# Build the project
cargo build

# Run tests
cargo test

# Run clippy
cargo clippy
```

## Making Changes

### Finding Issues to Work On

- Check the [Issues](https://github.com/Dicklesworthstone/xf/issues) page
- Look for issues labeled `good first issue` for beginner-friendly tasks
- Feel free to create an issue for bugs or features you'd like to work on

### Branch Naming

Use descriptive branch names:

- `feature/add-export-csv` - New features
- `fix/search-unicode-panic` - Bug fixes
- `docs/update-readme` - Documentation updates
- `refactor/simplify-parser` - Code refactoring

### Commit Messages

Write clear, descriptive commit messages:

```
<type>(<scope>): <subject>

<body>

<footer>
```

**Types:**
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation only
- `style`: Formatting, missing semicolons, etc.
- `refactor`: Code change that neither fixes a bug nor adds a feature
- `test`: Adding missing tests
- `chore`: Maintenance tasks

**Example:**
```
feat(search): add phrase query support

Implement phrase queries using Tantivy's position-based matching.
This allows users to search for exact phrases like "rust programming".

Closes #42
```

## Pull Request Process

1. **Fork the repository** and create your branch from `main`

2. **Make your changes** following the coding standards

3. **Add tests** for any new functionality

4. **Update documentation** if needed

5. **Run the full test suite:**
   ```bash
   cargo test --all-features
   cargo clippy --all-targets -- -D warnings
   cargo fmt -- --check
   ```

6. **Submit a pull request** with:
   - A clear title describing the change
   - A description of what was changed and why
   - Reference to any related issues

7. **Address review feedback** promptly

### PR Checklist

- [ ] Tests pass locally
- [ ] Code follows project style (rustfmt, clippy clean)
- [ ] Documentation updated if needed
- [ ] Commit messages are clear
- [ ] No unnecessary changes included

## Coding Standards

### Style Guide

- Follow standard Rust formatting (`cargo fmt`)
- No clippy warnings (`cargo clippy -- -D warnings`)
- Maximum line length: 100 characters
- Use `snake_case` for functions and variables
- Use `CamelCase` for types and traits
- Use `SCREAMING_SNAKE_CASE` for constants

### Error Handling

- Use the custom `XfError` type from `src/error.rs`
- Provide context for errors using `.context()` or custom variants
- Handle all `Result` types explicitly (no `.unwrap()` in production code)

```rust
// Good
let data = read_file(path).context("Failed to read archive")?;

// Avoid
let data = read_file(path).unwrap();
```

### Performance

- Follow performance budgets defined in `src/perf.rs`
- Use `Timer` to track operations in hot paths
- Benchmark before and after significant changes

```rust
use crate::perf::{self, Timer};

fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
    let _timer = Timer::new(perf::SEARCH_SIMPLE);
    // ... implementation
}
```

### Documentation

- All public items must have documentation comments
- Include examples for complex functions
- Update docs when changing behavior

```rust
/// Search the index for documents matching the query.
///
/// # Arguments
///
/// * `query` - Search query string
/// * `limit` - Maximum number of results
///
/// # Returns
///
/// Vector of search results, ordered by relevance
///
/// # Examples
///
/// ```rust
/// let results = engine.search("rust", 10)?;
/// ```
pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>
```

## Testing

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture

# Run ignored/slow tests
cargo test -- --ignored
```

### Writing Tests

- Place unit tests in the same file as the code
- Use descriptive test names
- Test both success and failure cases
- Use helper functions to reduce duplication

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_data() -> TestData {
        // ... helper function
    }

    #[test]
    fn test_search_returns_results() {
        let engine = SearchEngine::open_memory().unwrap();
        // ... test implementation
    }

    #[test]
    fn test_search_handles_empty_query() {
        // ... edge case test
    }
}
```

### Benchmarks

```bash
# Run benchmarks
cargo bench

# Run specific benchmark
cargo bench --bench search_perf -- search_simple
```

## Documentation

### Updating Documentation

- `README.md` - Project overview and quick start
- `docs/architecture.md` - System design and architecture
- `docs/performance.md` - Performance characteristics
- `docs/troubleshooting.md` - Common issues and solutions
- `docs/development.md` - Development guide

### Building Documentation

```bash
# Build and open rustdoc
cargo doc --open

# Check documentation for warnings
cargo doc --no-deps 2>&1 | grep -i warning
```

## Release Process

Releases are automated via GitHub Actions when a tag is pushed:

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md`
3. Commit changes
4. Create and push tag:
   ```bash
   git tag -a v0.2.0 -m "Release v0.2.0"
   git push origin v0.2.0
   ```

## Questions?

- Open an issue for questions or discussion
- Check existing issues and documentation first

Thank you for contributing!
