# xf Troubleshooting Guide

This guide helps diagnose and resolve common issues with xf.

## Quick Diagnostics

### Check xf Status

```bash
# Version and build info
xf --version

# Verify database exists
ls -la ~/.local/share/xf/

# Check database integrity
sqlite3 ~/.local/share/xf/xf.db "PRAGMA integrity_check"

# Count indexed records
xf stats
```

## Common Issues

### 1. "Archive not found" Error

**Error Message:**
```
Error: Archive not found at '/path/to/archive'
```

**Causes:**
- Path doesn't exist
- Path is a file, not a directory
- Missing required archive files

**Solutions:**

1. Verify the path exists:
   ```bash
   ls -la /path/to/archive
   ```

2. Check for required files:
   ```bash
   # X archives should contain:
   ls /path/to/archive/data/
   # Expected: tweets.js, like.js, etc.
   ```

3. If using a ZIP file, extract it first:
   ```bash
   unzip twitter-archive.zip -d ~/twitter-archive
   xf index ~/twitter-archive
   ```

### 2. "Database is locked" Error

**Error Message:**
```
Error: Database error: database is locked
```

**Causes:**
- Another xf process is running
- Previous process crashed and left a lock
- Database on network filesystem (NFS/SMB)

**Solutions:**

1. Check for running xf processes:
   ```bash
   ps aux | grep xf
   ```

2. Remove stale locks:
   ```bash
   rm ~/.local/share/xf/xf.db-wal
   rm ~/.local/share/xf/xf.db-shm
   ```

3. Move database to local filesystem:
   ```bash
   export XF_DB=~/xf.db
   export XF_INDEX=~/xf_index
   xf index ~/archive
   ```

### 3. "No index found" Error

**Error Message:**
```
Error: Index not found at '~/.local/share/xf/xf_index'
```

**Causes:**
- Never ran `xf index`
- Index was deleted or moved
- Custom index path not set

**Solutions:**

1. Run indexing:
   ```bash
   xf index ~/path/to/archive
   ```

2. Check custom path settings:
   ```bash
   echo $XF_INDEX
   cat ~/.config/xf/config.toml
   ```

3. Rebuild index:
   ```bash
   xf index --force ~/path/to/archive
   ```

### 4. "Invalid UTF-8" Error

**Error Message:**
```
Error: Parse error: Invalid UTF-8 sequence in data
```

**Causes:**
- Corrupted archive files
- Non-UTF-8 encoding in export
- Binary data in text fields

**Solutions:**

1. Check file encoding:
   ```bash
   file ~/archive/data/tweets.js
   ```

2. Convert to UTF-8 if needed:
   ```bash
   iconv -f LATIN1 -t UTF-8 tweets.js > tweets_utf8.js
   ```

3. Re-export from X if possible

### 5. Slow Search Performance

**Symptom:** Searches take >100ms consistently.

**Causes:**
- Very large corpus
- Complex queries
- Cold cache
- Resource constraints

**Solutions:**

1. Check corpus size:
   ```bash
   xf stats
   ```

2. Simplify query:
   ```bash
   # Instead of broad queries
   xf search "the"

   # Use specific terms
   xf search "rust programming"
   ```

3. Use type filters:
   ```bash
   xf search "query" --types tweet
   ```

4. Warm the cache:
   ```bash
   xf search "warmup" > /dev/null
   ```

### 6. High Memory Usage

**Symptom:** xf uses several GB of RAM.

**Causes:**
- Large buffer size during indexing
- Many parallel threads
- Large search result sets

**Solutions:**

1. Reduce buffer size:
   ```bash
   XF_BUFFER_MB=64 xf index ~/archive
   ```

2. Limit threads:
   ```bash
   XF_THREADS=2 xf index ~/archive
   ```

3. Limit results:
   ```bash
   xf search "query" --limit 20
   ```

### 7. Missing Data After Indexing

**Symptom:** Some tweets/DMs not appearing in search.

**Causes:**
- Partial archive export
- Parsing errors on specific records
- Type filters excluding data

**Solutions:**

1. Check statistics:
   ```bash
   xf stats
   # Compare counts to expected
   ```

2. Check for parse warnings:
   ```bash
   xf index --verbose ~/archive 2>&1 | grep -i warn
   ```

3. Verify archive completeness:
   ```bash
   ls -la ~/archive/data/*.js | wc -l
   ```

4. Re-export from X with all data selected

### 8. "Permission denied" Error

**Error Message:**
```
Error: IO error: Permission denied
```

**Causes:**
- No write access to data directory
- Database owned by different user
- SELinux/AppArmor restrictions

**Solutions:**

1. Check permissions:
   ```bash
   ls -la ~/.local/share/xf/
   ```

2. Fix ownership:
   ```bash
   chown -R $USER ~/.local/share/xf/
   ```

3. Use custom path:
   ```bash
   export XF_DB=~/my-xf/xf.db
   export XF_INDEX=~/my-xf/index
   ```

### 9. Search Returns No Results

**Symptom:** Queries return empty results.

**Causes:**
- Index not built or outdated
- Incorrect query syntax
- Type filters too restrictive

**Solutions:**

1. Verify index exists:
   ```bash
   xf stats
   ```

2. Try simpler query:
   ```bash
   xf search "*"  # Should return something
   ```

3. Check type filters:
   ```bash
   # Remove type restriction
   xf search "query"  # Not --types
   ```

4. Rebuild index:
   ```bash
   xf index --force ~/archive
   ```

### 10. Corrupted Index

**Error Message:**
```
Error: Search engine error: Corrupted index segment
```

**Causes:**
- Interrupted indexing
- Disk full during write
- Hardware issues

**Solutions:**

1. Remove and rebuild index:
   ```bash
   rm -rf ~/.local/share/xf/xf_index
   xf index ~/archive
   ```

2. Check disk space:
   ```bash
   df -h ~/.local/share/
   ```

3. Run filesystem check if issues persist

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `XF_DB` | Database file path | `~/.local/share/xf/xf.db` |
| `XF_INDEX` | Index directory path | `~/.local/share/xf/xf_index` |
| `XF_ARCHIVE` | Default archive path | (none) |
| `XF_LIMIT` | Default result limit | 20 |
| `XF_FORMAT` | Output format | text |
| `XF_BUFFER_MB` | Indexing buffer size | 256 |
| `XF_THREADS` | Thread count (0=auto) | 0 |
| `XF_NO_COLOR` | Disable colors | (unset) |
| `XF_QUIET` | Suppress progress | (unset) |

## Debug Mode

Enable verbose logging for troubleshooting:

```bash
# Set log level
RUST_LOG=xf=debug xf search "query"

# Very verbose
RUST_LOG=xf=trace xf search "query"

# Log to file
RUST_LOG=xf=debug xf search "query" 2> debug.log
```

## Getting Help

If issues persist:

1. Check existing issues: https://github.com/Dicklesworthstone/xf/issues
2. Create a new issue with:
   - xf version (`xf --version`)
   - OS and version
   - Complete error message
   - Steps to reproduce
   - Debug log output

## Recovery Procedures

### Complete Reset

```bash
# Remove all xf data
rm -rf ~/.local/share/xf/
rm -rf ~/.config/xf/

# Reinstall and reindex
xf index ~/archive
```

### Database Recovery

```bash
# Backup current database
cp ~/.local/share/xf/xf.db ~/.local/share/xf/xf.db.bak

# Export and reimport
sqlite3 ~/.local/share/xf/xf.db ".dump" > backup.sql

# Create fresh database
rm ~/.local/share/xf/xf.db
xf index ~/archive
```

### Index-Only Recovery

```bash
# Keep database, rebuild search index
rm -rf ~/.local/share/xf/xf_index
xf index --rebuild-index ~/archive
```
