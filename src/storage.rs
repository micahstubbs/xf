//! `SQLite` storage for X archive data.
//!
//! Provides persistent storage with optimized schema for fast queries.

use crate::doctor::{CheckCategory, CheckStatus, HealthCheck, TableStat};
use crate::model::{
    ArchiveInfo, ArchiveStats, Block, DirectMessage, DmConversation, DmConversationSummary,
    Follower, Following, GrokMessage, Like, Mute, Tweet,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use std::collections::{HashSet, VecDeque};
use std::path::Path;
use tracing::info;

const SCHEMA_VERSION: i32 = 1;
const BYTES_PER_KB: u64 = 1024;
const BYTES_PER_MB: u64 = 1024 * 1024;
const BYTES_PER_GB: u64 = 1024 * 1024 * 1024;

const fn epoch_utc() -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(0, 0).unwrap()
}

fn parse_rfc3339_or_epoch(value: Option<String>) -> DateTime<Utc> {
    value
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map_or_else(epoch_utc, |dt| dt.with_timezone(&Utc))
}

fn parse_rfc3339_opt(value: Option<String>) -> Option<DateTime<Utc>> {
    value
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

/// `SQLite` storage manager
pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// Open or create the database at the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or initialized.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(db_path.as_ref()).with_context(|| {
            format!("Failed to open database at {}", db_path.as_ref().display())
        })?;

        // Set pragmas for performance
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA cache_size = -64000;
            PRAGMA temp_store = MEMORY;
            ",
        )?;

        let storage = Self { conn };
        storage.migrate()?;
        Ok(storage)
    }

    /// Open an in-memory database (for testing).
    ///
    /// # Errors
    ///
    /// Returns an error if the in-memory database cannot be initialized.
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA temp_store = MEMORY;
            ",
        )?;
        let storage = Self { conn };
        storage.migrate()?;
        Ok(storage)
    }

    /// Get a reference to the underlying database connection.
    ///
    /// This is useful for modules that need to execute custom queries.
    #[must_use]
    pub const fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Run database migrations
    fn migrate(&self) -> Result<()> {
        let current_version = self.get_schema_version();

        if current_version < SCHEMA_VERSION {
            info!(
                "Migrating database from version {} to {}",
                current_version, SCHEMA_VERSION
            );
            self.create_schema()?;
            self.set_schema_version(SCHEMA_VERSION)?;
        }

        Ok(())
    }

    fn get_schema_version(&self) -> i32 {
        let result: Result<i32, _> = self.conn.query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| {
                let value: String = row.get(0)?;
                Ok(value.parse().unwrap_or(0))
            },
        );

        // Treat missing schema table as version 0.
        result.unwrap_or_default()
    }

    fn set_schema_version(&self, version: i32) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?)",
            params![version.to_string()],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn create_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r"
            -- Metadata table
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            -- Archive info
            CREATE TABLE IF NOT EXISTS archive_info (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                account_id TEXT NOT NULL,
                username TEXT NOT NULL,
                display_name TEXT,
                archive_size_bytes INTEGER,
                generation_date TEXT,
                is_partial INTEGER DEFAULT 0,
                indexed_at TEXT NOT NULL
            );

            -- Tweets
            CREATE TABLE IF NOT EXISTS tweets (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                full_text TEXT NOT NULL,
                source TEXT,
                favorite_count INTEGER DEFAULT 0,
                retweet_count INTEGER DEFAULT 0,
                lang TEXT,
                in_reply_to_status_id TEXT,
                in_reply_to_user_id TEXT,
                in_reply_to_screen_name TEXT,
                is_retweet INTEGER DEFAULT 0,
                hashtags_json TEXT,
                mentions_json TEXT,
                urls_json TEXT,
                media_json TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_tweets_created_at ON tweets(created_at);
            CREATE INDEX IF NOT EXISTS idx_tweets_in_reply_to ON tweets(in_reply_to_status_id);

            -- Likes
            CREATE TABLE IF NOT EXISTS likes (
                tweet_id TEXT PRIMARY KEY,
                full_text TEXT,
                expanded_url TEXT
            );

            -- DM Conversations
            CREATE TABLE IF NOT EXISTS dm_conversations (
                conversation_id TEXT PRIMARY KEY,
                participant_ids TEXT,
                message_count INTEGER DEFAULT 0,
                first_message_at TEXT,
                last_message_at TEXT
            );

            -- Direct Messages
            CREATE TABLE IF NOT EXISTS direct_messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                recipient_id TEXT NOT NULL,
                text TEXT NOT NULL,
                created_at TEXT NOT NULL,
                urls_json TEXT,
                media_urls_json TEXT,
                FOREIGN KEY (conversation_id) REFERENCES dm_conversations(conversation_id)
            );
            CREATE INDEX IF NOT EXISTS idx_dm_conversation ON direct_messages(conversation_id);
            CREATE INDEX IF NOT EXISTS idx_dm_created_at ON direct_messages(created_at);

            -- Followers
            CREATE TABLE IF NOT EXISTS followers (
                account_id TEXT PRIMARY KEY,
                user_link TEXT
            );

            -- Following
            CREATE TABLE IF NOT EXISTS following (
                account_id TEXT PRIMARY KEY,
                user_link TEXT
            );

            -- Blocks
            CREATE TABLE IF NOT EXISTS blocks (
                account_id TEXT PRIMARY KEY,
                user_link TEXT
            );

            -- Mutes
            CREATE TABLE IF NOT EXISTS mutes (
                account_id TEXT PRIMARY KEY,
                user_link TEXT
            );

            -- Grok messages
            CREATE TABLE IF NOT EXISTS grok_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id TEXT NOT NULL,
                message TEXT NOT NULL,
                sender TEXT NOT NULL,
                created_at TEXT NOT NULL,
                grok_mode TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_grok_chat_id ON grok_messages(chat_id);
            CREATE INDEX IF NOT EXISTS idx_grok_created_at ON grok_messages(created_at);

            -- Full-text search virtual tables (standalone, not content-synced)
            CREATE VIRTUAL TABLE IF NOT EXISTS fts_tweets USING fts5(
                tweet_id,
                full_text
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS fts_likes USING fts5(
                tweet_id,
                full_text
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS fts_dms USING fts5(
                dm_id,
                text
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS fts_grok USING fts5(
                grok_id,
                message
            );
            ",
        )?;

        Ok(())
    }

    /// Store archive info.
    ///
    /// # Errors
    ///
    /// Returns an error if the database insert fails.
    pub fn store_archive_info(&self, info: &ArchiveInfo) -> Result<()> {
        self.conn.execute(
            r"
            INSERT OR REPLACE INTO archive_info
            (id, account_id, username, display_name, archive_size_bytes, generation_date, is_partial, indexed_at)
            VALUES (1, ?, ?, ?, ?, ?, ?, ?)
            ",
            params![
                info.account_id,
                info.username,
                info.display_name,
                info.archive_size_bytes,
                info.generation_date.to_rfc3339(),
                i32::from(info.is_partial),
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Store tweets in a transaction.
    ///
    /// # Errors
    ///
    /// Returns an error if any tweet insert fails.
    pub fn store_tweets(&mut self, tweets: &[Tweet]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;

        {
            // FTS5 doesn't support INSERT OR REPLACE, so we must delete first to avoid duplicates.
            // Batch delete for performance: one DELETE with IN clause instead of N individual DELETEs.
            if !tweets.is_empty() {
                let placeholders: String = tweets.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let delete_sql =
                    format!("DELETE FROM fts_tweets WHERE tweet_id IN ({placeholders})");
                let mut delete_stmt = tx.prepare(&delete_sql)?;
                delete_stmt.execute(rusqlite::params_from_iter(tweets.iter().map(|t| &t.id)))?;
            }

            let mut stmt = tx.prepare(
                r"
                INSERT OR REPLACE INTO tweets
                (id, created_at, full_text, source, favorite_count, retweet_count, lang,
                 in_reply_to_status_id, in_reply_to_user_id, in_reply_to_screen_name,
                 is_retweet, hashtags_json, mentions_json, urls_json, media_json)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ",
            )?;
            let mut fts_stmt =
                tx.prepare("INSERT INTO fts_tweets (tweet_id, full_text) VALUES (?, ?)")?;

            for tweet in tweets {
                stmt.execute(params![
                    tweet.id,
                    tweet.created_at.to_rfc3339(),
                    tweet.full_text,
                    tweet.source,
                    tweet.favorite_count,
                    tweet.retweet_count,
                    tweet.lang,
                    tweet.in_reply_to_status_id,
                    tweet.in_reply_to_user_id,
                    tweet.in_reply_to_screen_name,
                    i32::from(tweet.is_retweet),
                    serde_json::to_string(&tweet.hashtags)?,
                    serde_json::to_string(&tweet.user_mentions)?,
                    serde_json::to_string(&tweet.urls)?,
                    serde_json::to_string(&tweet.media)?,
                ])?;
                fts_stmt.execute(params![&tweet.id, &tweet.full_text])?;
                count += 1;
            }
        }

        tx.commit()?;
        info!("Stored {} tweets", count);
        Ok(count)
    }

    /// Store likes in a transaction.
    ///
    /// # Errors
    ///
    /// Returns an error if any like insert fails.
    pub fn store_likes(&mut self, likes: &[Like]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;

        {
            // FTS5 batch delete for likes with text
            let likes_with_text: Vec<_> = likes.iter().filter(|l| l.full_text.is_some()).collect();
            if !likes_with_text.is_empty() {
                let placeholders: String = likes_with_text
                    .iter()
                    .map(|_| "?")
                    .collect::<Vec<_>>()
                    .join(",");
                let delete_sql =
                    format!("DELETE FROM fts_likes WHERE tweet_id IN ({placeholders})");
                let mut delete_stmt = tx.prepare(&delete_sql)?;
                delete_stmt.execute(rusqlite::params_from_iter(
                    likes_with_text.iter().map(|l| &l.tweet_id),
                ))?;
            }

            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO likes (tweet_id, full_text, expanded_url) VALUES (?, ?, ?)",
            )?;
            let mut fts_stmt =
                tx.prepare("INSERT INTO fts_likes (tweet_id, full_text) VALUES (?, ?)")?;

            for like in likes {
                stmt.execute(params![like.tweet_id, like.full_text, like.expanded_url])?;
                if let Some(text) = &like.full_text {
                    fts_stmt.execute(params![&like.tweet_id, text])?;
                }
                count += 1;
            }
        }

        tx.commit()?;
        info!("Stored {} likes", count);
        Ok(count)
    }

    /// Store DM conversations and messages.
    ///
    /// # Errors
    ///
    /// Returns an error if any conversation or message insert fails.
    pub fn store_dm_conversations(&mut self, conversations: &[DmConversation]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut message_count = 0;

        {
            // Collect all message IDs for batch FTS delete
            let all_msg_ids: Vec<&str> = conversations
                .iter()
                .flat_map(|c| c.messages.iter().map(|m| m.id.as_str()))
                .collect();
            if !all_msg_ids.is_empty() {
                let placeholders: String = all_msg_ids
                    .iter()
                    .map(|_| "?")
                    .collect::<Vec<_>>()
                    .join(",");
                let delete_sql = format!("DELETE FROM fts_dms WHERE dm_id IN ({placeholders})");
                let mut delete_stmt = tx.prepare(&delete_sql)?;
                delete_stmt.execute(rusqlite::params_from_iter(all_msg_ids.iter()))?;
            }

            let mut conv_stmt = tx.prepare(
                r"
                INSERT OR REPLACE INTO dm_conversations
                (conversation_id, participant_ids, message_count, first_message_at, last_message_at)
                VALUES (?, ?, ?, ?, ?)
                ",
            )?;

            let mut msg_stmt = tx.prepare(
                r"
                INSERT OR REPLACE INTO direct_messages
                (id, conversation_id, sender_id, recipient_id, text, created_at, urls_json, media_urls_json)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ",
            )?;

            let mut fts_stmt = tx.prepare("INSERT INTO fts_dms (dm_id, text) VALUES (?, ?)")?;

            for conv in conversations {
                // Get participant IDs and date range
                let mut participant_ids: Vec<&str> = conv
                    .messages
                    .iter()
                    .flat_map(|m| vec![m.sender_id.as_str(), m.recipient_id.as_str()])
                    .collect();
                participant_ids.sort_unstable();
                participant_ids.dedup();

                let first_msg = conv.messages.iter().min_by_key(|m| m.created_at);
                let last_msg = conv.messages.iter().max_by_key(|m| m.created_at);

                conv_stmt.execute(params![
                    conv.conversation_id,
                    participant_ids.join(","),
                    i64::try_from(conv.messages.len()).unwrap_or(i64::MAX),
                    first_msg.map(|m| m.created_at.to_rfc3339()),
                    last_msg.map(|m| m.created_at.to_rfc3339()),
                ])?;

                for msg in &conv.messages {
                    msg_stmt.execute(params![
                        msg.id,
                        conv.conversation_id,
                        msg.sender_id,
                        msg.recipient_id,
                        msg.text,
                        msg.created_at.to_rfc3339(),
                        serde_json::to_string(&msg.urls)?,
                        serde_json::to_string(&msg.media_urls)?,
                    ])?;
                    fts_stmt.execute(params![&msg.id, &msg.text])?;
                    message_count += 1;
                }
            }
        }
        tx.commit()?;
        info!(
            "Stored {} conversations with {} messages",
            conversations.len(),
            message_count
        );
        Ok(message_count)
    }

    /// Store followers.
    ///
    /// # Errors
    ///
    /// Returns an error if any follower insert fails.
    pub fn store_followers(&mut self, followers: &[Follower]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;

        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO followers (account_id, user_link) VALUES (?, ?)",
            )?;

            for f in followers {
                stmt.execute(params![f.account_id, f.user_link])?;
                count += 1;
            }
        }

        tx.commit()?;
        info!("Stored {} followers", count);
        Ok(count)
    }

    /// Store following.
    ///
    /// # Errors
    ///
    /// Returns an error if any following insert fails.
    pub fn store_following(&mut self, following: &[Following]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;

        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO following (account_id, user_link) VALUES (?, ?)",
            )?;

            for f in following {
                stmt.execute(params![f.account_id, f.user_link])?;
                count += 1;
            }
        }

        tx.commit()?;
        info!("Stored {} following", count);
        Ok(count)
    }

    /// Store blocks.
    ///
    /// # Errors
    ///
    /// Returns an error if any block insert fails.
    pub fn store_blocks(&mut self, blocks: &[Block]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;

        {
            let mut stmt =
                tx.prepare("INSERT OR REPLACE INTO blocks (account_id, user_link) VALUES (?, ?)")?;

            for b in blocks {
                stmt.execute(params![b.account_id, b.user_link])?;
                count += 1;
            }
        }

        tx.commit()?;
        info!("Stored {} blocks", count);
        Ok(count)
    }

    /// Store mutes.
    ///
    /// # Errors
    ///
    /// Returns an error if any mute insert fails.
    pub fn store_mutes(&mut self, mutes: &[Mute]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;

        {
            let mut stmt =
                tx.prepare("INSERT OR REPLACE INTO mutes (account_id, user_link) VALUES (?, ?)")?;

            for m in mutes {
                stmt.execute(params![m.account_id, m.user_link])?;
                count += 1;
            }
        }

        tx.commit()?;
        info!("Stored {} mutes", count);
        Ok(count)
    }

    /// Store Grok messages.
    ///
    /// # Errors
    ///
    /// Returns an error if any Grok message insert fails.
    pub fn store_grok_messages(&mut self, messages: &[GrokMessage]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;

        {
            // Compute grok_ids for batch FTS delete
            let grok_ids: Vec<String> = messages
                .iter()
                .map(|msg| {
                    format!(
                        "{}_{}_{}_{}",
                        msg.chat_id,
                        msg.created_at.timestamp(),
                        msg.created_at.timestamp_subsec_nanos(),
                        msg.sender
                    )
                })
                .collect();

            if !grok_ids.is_empty() {
                let placeholders: String =
                    grok_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let delete_sql = format!("DELETE FROM fts_grok WHERE grok_id IN ({placeholders})");
                let mut delete_stmt = tx.prepare(&delete_sql)?;
                delete_stmt.execute(rusqlite::params_from_iter(grok_ids.iter()))?;
            }

            let mut stmt = tx.prepare(
                r"
                INSERT INTO grok_messages (chat_id, message, sender, created_at, grok_mode)
                VALUES (?, ?, ?, ?, ?)
                ",
            )?;
            let mut fts_stmt =
                tx.prepare("INSERT INTO fts_grok (grok_id, message) VALUES (?, ?)")?;

            for (msg, grok_id) in messages.iter().zip(grok_ids.iter()) {
                stmt.execute(params![
                    msg.chat_id,
                    msg.message,
                    msg.sender,
                    msg.created_at.to_rfc3339(),
                    msg.grok_mode,
                ])?;
                fts_stmt.execute(params![grok_id, &msg.message])?;
                count += 1;
            }
        }

        tx.commit()?;
        info!("Stored {} Grok messages", count);
        Ok(count)
    }

    /// Get archive statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if statistics queries fail.
    pub fn get_stats(&self) -> Result<ArchiveStats> {
        let tweets_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM tweets", [], |row| row.get(0))?;

        let likes_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM likes", [], |row| row.get(0))?;

        let dms_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM direct_messages", [], |row| row.get(0))?;

        let dm_conversations_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM dm_conversations", [], |row| {
                    row.get(0)
                })?;

        let followers_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM followers", [], |row| row.get(0))?;

        let following_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM following", [], |row| row.get(0))?;

        let blocks_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM blocks", [], |row| row.get(0))?;

        let mutes_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM mutes", [], |row| row.get(0))?;

        let grok_messages_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM grok_messages", [], |row| row.get(0))?;

        let first_tweet_date: Option<String> = self
            .conn
            .query_row("SELECT MIN(created_at) FROM tweets", [], |row| row.get(0))
            .ok();

        let last_tweet_date: Option<String> = self
            .conn
            .query_row("SELECT MAX(created_at) FROM tweets", [], |row| row.get(0))
            .ok();

        Ok(ArchiveStats {
            tweets_count,
            likes_count,
            dms_count,
            dm_conversations_count,
            followers_count,
            following_count,
            blocks_count,
            mutes_count,
            grok_messages_count,
            first_tweet_date: first_tweet_date
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            last_tweet_date: last_tweet_date
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            index_built_at: Utc::now(),
        })
    }

    /// Get the count of documents expected in the Tantivy index.
    ///
    /// # Errors
    ///
    /// Returns an error if the count queries fail.
    pub fn indexable_document_count(&self) -> Result<i64> {
        let tweets_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM tweets", [], |row| row.get(0))?;

        let likes_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM likes WHERE full_text IS NOT NULL AND full_text != ''",
            [],
            |row| row.get(0),
        )?;

        let dms_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM direct_messages", [], |row| row.get(0))?;

        let grok_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM grok_messages", [], |row| row.get(0))?;

        Ok(tweets_count + likes_count + dms_count + grok_count)
    }

    /// Run database health checks for `xf doctor`.
    #[must_use]
    pub fn database_health_checks(&self) -> Vec<HealthCheck> {
        let mut checks = Vec::new();

        checks.push(self.check_integrity());
        checks.push(self.check_schema_version());
        checks.extend(self.check_fts_integrity());
        checks.extend(self.check_fts_orphaned());
        checks.extend(self.check_fts_missing());
        checks.push(self.check_orphaned_dm_messages());
        checks.push(self.check_grok_fts_counts());
        checks.push(self.check_table_stats());

        checks
    }

    /// Collect per-table row counts and optional size statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if a table count query fails.
    pub fn database_table_stats(&self) -> Result<Vec<TableStat>> {
        let tables = [
            "tweets",
            "likes",
            "direct_messages",
            "dm_conversations",
            "grok_messages",
            "followers",
            "following",
            "blocks",
            "mutes",
            "fts_tweets",
            "fts_likes",
            "fts_dms",
            "fts_grok",
        ];

        let has_dbstat = self.dbstat_available();
        let mut stats = Vec::with_capacity(tables.len());

        for table in tables {
            let rows = self.table_row_count(table)?;
            let bytes = if has_dbstat {
                self.table_size_bytes(table)
            } else {
                None
            };
            stats.push(TableStat {
                name: table.to_string(),
                rows,
                bytes,
            });
        }

        Ok(stats)
    }

    fn check_integrity(&self) -> HealthCheck {
        match self
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
        {
            Ok(result) => {
                if result == "ok" {
                    HealthCheck {
                        category: CheckCategory::Database,
                        name: "PRAGMA integrity_check".to_string(),
                        status: CheckStatus::Pass,
                        message: "ok".to_string(),
                        suggestion: None,
                    }
                } else {
                    HealthCheck {
                        category: CheckCategory::Database,
                        name: "PRAGMA integrity_check".to_string(),
                        status: CheckStatus::Error,
                        message: format!("Integrity check failed: {result}"),
                        suggestion: Some(
                            "Database corruption detected. Re-index or restore from backup."
                                .to_string(),
                        ),
                    }
                }
            }
            Err(err) => HealthCheck {
                category: CheckCategory::Database,
                name: "PRAGMA integrity_check".to_string(),
                status: CheckStatus::Error,
                message: format!("Integrity check failed: {err}"),
                suggestion: Some("Re-index the archive to rebuild the database.".to_string()),
            },
        }
    }

    fn check_schema_version(&self) -> HealthCheck {
        let current = self.get_schema_version();
        if current == SCHEMA_VERSION {
            HealthCheck {
                category: CheckCategory::Database,
                name: "Schema version".to_string(),
                status: CheckStatus::Pass,
                message: format!("schema_version={current}"),
                suggestion: None,
            }
        } else {
            HealthCheck {
                category: CheckCategory::Database,
                name: "Schema version".to_string(),
                status: CheckStatus::Error,
                message: format!("schema_version={current}, expected={SCHEMA_VERSION}"),
                suggestion: Some("Run 'xf index --force' to rebuild the database.".to_string()),
            }
        }
    }

    fn check_fts_integrity(&self) -> Vec<HealthCheck> {
        let tables = ["fts_tweets", "fts_likes", "fts_dms", "fts_grok"];
        let mut checks = Vec::with_capacity(tables.len());

        for table in tables {
            let sql = format!("INSERT INTO {table}({table}) VALUES('integrity-check')");
            let result = self.conn.execute(&sql, []);
            let (status, message, suggestion) = match result {
                Ok(_) => (CheckStatus::Pass, "ok".to_string(), None),
                Err(err) => (
                    CheckStatus::Error,
                    format!("Integrity check failed: {err}"),
                    Some("Run 'xf index --force' to rebuild FTS tables.".to_string()),
                ),
            };

            checks.push(HealthCheck {
                category: CheckCategory::Database,
                name: format!("FTS5 integrity ({table})"),
                status,
                message,
                suggestion,
            });
        }

        checks
    }

    fn check_fts_orphaned(&self) -> Vec<HealthCheck> {
        vec![
            self.check_count(
                "FTS orphaned rows (tweets)",
                "SELECT COUNT(*) FROM fts_tweets fts LEFT JOIN tweets t ON fts.tweet_id = t.id WHERE t.id IS NULL",
                "Run 'xf index --force' to rebuild FTS tables.",
            ),
            self.check_count(
                "FTS orphaned rows (likes)",
                "SELECT COUNT(*) FROM fts_likes fts LEFT JOIN likes l ON fts.tweet_id = l.tweet_id WHERE l.tweet_id IS NULL",
                "Run 'xf index --force' to rebuild FTS tables.",
            ),
            self.check_count(
                "FTS orphaned rows (dms)",
                "SELECT COUNT(*) FROM fts_dms fts LEFT JOIN direct_messages dm ON fts.dm_id = dm.id WHERE dm.id IS NULL",
                "Run 'xf index --force' to rebuild FTS tables.",
            ),
        ]
    }

    fn check_fts_missing(&self) -> Vec<HealthCheck> {
        vec![
            self.check_count(
                "FTS missing rows (tweets)",
                "SELECT COUNT(*) FROM tweets t LEFT JOIN fts_tweets fts ON fts.tweet_id = t.id WHERE fts.tweet_id IS NULL",
                "Run 'xf index --force' to rebuild FTS tables.",
            ),
            self.check_count(
                "FTS missing rows (likes)",
                "SELECT COUNT(*) FROM likes l LEFT JOIN fts_likes fts ON fts.tweet_id = l.tweet_id WHERE l.full_text IS NOT NULL AND fts.tweet_id IS NULL",
                "Run 'xf index --force' to rebuild FTS tables.",
            ),
            self.check_count(
                "FTS missing rows (dms)",
                "SELECT COUNT(*) FROM direct_messages dm LEFT JOIN fts_dms fts ON fts.dm_id = dm.id WHERE fts.dm_id IS NULL",
                "Run 'xf index --force' to rebuild FTS tables.",
            ),
        ]
    }

    fn check_orphaned_dm_messages(&self) -> HealthCheck {
        self.check_count(
            "Orphaned DM messages",
            "SELECT COUNT(*) FROM direct_messages dm LEFT JOIN dm_conversations conv ON dm.conversation_id = conv.conversation_id WHERE conv.conversation_id IS NULL",
            "Run 'xf index --force' to rebuild DM conversations.",
        )
    }

    fn check_grok_fts_counts(&self) -> HealthCheck {
        let grok_count = self.table_row_count("grok_messages");
        let fts_count = self.table_row_count("fts_grok");

        match (grok_count, fts_count) {
            (Ok(grok_rows), Ok(fts_rows)) => {
                if grok_rows == fts_rows {
                    HealthCheck {
                        category: CheckCategory::Database,
                        name: "FTS row count (grok)".to_string(),
                        status: CheckStatus::Pass,
                        message: format!("grok_messages={grok_rows}, fts_grok={fts_rows}"),
                        suggestion: None,
                    }
                } else {
                    HealthCheck {
                        category: CheckCategory::Database,
                        name: "FTS row count (grok)".to_string(),
                        status: CheckStatus::Warning,
                        message: format!("grok_messages={grok_rows}, fts_grok={fts_rows}"),
                        suggestion: Some("Run 'xf index --force' to rebuild Grok FTS.".to_string()),
                    }
                }
            }
            (Err(err), _) | (_, Err(err)) => HealthCheck {
                category: CheckCategory::Database,
                name: "FTS row count (grok)".to_string(),
                status: CheckStatus::Error,
                message: format!("Failed to read row counts: {err}"),
                suggestion: Some("Run 'xf index --force' to rebuild Grok FTS.".to_string()),
            },
        }
    }

    fn check_table_stats(&self) -> HealthCheck {
        match self.database_table_stats() {
            Ok(stats) => {
                let message = format_table_stats(&stats);
                HealthCheck {
                    category: CheckCategory::Database,
                    name: "Table stats".to_string(),
                    status: CheckStatus::Pass,
                    message,
                    suggestion: None,
                }
            }
            Err(err) => HealthCheck {
                category: CheckCategory::Database,
                name: "Table stats".to_string(),
                status: CheckStatus::Error,
                message: format!("Failed to collect table stats: {err}"),
                suggestion: None,
            },
        }
    }

    fn check_count(&self, name: &str, sql: &str, suggestion: &str) -> HealthCheck {
        match self.conn.query_row(sql, [], |row| row.get::<_, i64>(0)) {
            Ok(count) => {
                if count == 0 {
                    HealthCheck {
                        category: CheckCategory::Database,
                        name: name.to_string(),
                        status: CheckStatus::Pass,
                        message: "0 rows".to_string(),
                        suggestion: None,
                    }
                } else {
                    HealthCheck {
                        category: CheckCategory::Database,
                        name: name.to_string(),
                        status: CheckStatus::Warning,
                        message: format!("{count} rows"),
                        suggestion: Some(suggestion.to_string()),
                    }
                }
            }
            Err(err) => HealthCheck {
                category: CheckCategory::Database,
                name: name.to_string(),
                status: CheckStatus::Error,
                message: format!("Query failed: {err}"),
                suggestion: Some(suggestion.to_string()),
            },
        }
    }

    fn table_row_count(&self, table: &str) -> Result<i64> {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        self.conn
            .query_row(&sql, [], |row| row.get(0))
            .with_context(|| format!("Failed to count rows for table {table}"))
    }

    fn dbstat_available(&self) -> bool {
        let result: std::result::Result<i64, _> = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='dbstat'",
            [],
            |row| row.get(0),
        );
        result.unwrap_or(0) > 0
    }

    fn table_size_bytes(&self, table: &str) -> Option<i64> {
        let sql = "SELECT SUM(pgsize) FROM dbstat WHERE name = ?1";
        self.conn
            .query_row(sql, [table], |row| row.get::<_, Option<i64>>(0))
            .ok()
            .flatten()
    }

    /// Search tweets using FTS5.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn search_tweets(&self, query: &str, limit: usize) -> Result<Vec<Tweet>> {
        let limit = limit_to_i64(limit);
        let mut stmt = self.conn.prepare(
            r"
            SELECT t.id, t.created_at, t.full_text, t.source, t.favorite_count, t.retweet_count,
                   t.lang, t.in_reply_to_status_id, t.in_reply_to_user_id, t.in_reply_to_screen_name,
                   t.is_retweet, t.hashtags_json, t.mentions_json, t.urls_json, t.media_json
            FROM tweets t
            JOIN fts_tweets fts ON t.id = fts.tweet_id
            WHERE fts_tweets MATCH ?
            ORDER BY rank
            LIMIT ?
            ",
        )?;

        let tweets = stmt
            .query_map(params![query, limit], |row| {
                Ok(Tweet {
                    id: row.get(0)?,
                    created_at: parse_rfc3339_or_epoch(row.get::<_, Option<String>>(1)?),
                    full_text: row.get(2)?,
                    source: row.get(3)?,
                    favorite_count: row.get(4)?,
                    retweet_count: row.get(5)?,
                    lang: row.get(6)?,
                    in_reply_to_status_id: row.get(7)?,
                    in_reply_to_user_id: row.get(8)?,
                    in_reply_to_screen_name: row.get(9)?,
                    is_retweet: row.get::<_, i32>(10)? != 0,
                    hashtags: serde_json::from_str(&row.get::<_, String>(11)?).unwrap_or_default(),
                    user_mentions: serde_json::from_str(&row.get::<_, String>(12)?)
                        .unwrap_or_default(),
                    urls: serde_json::from_str(&row.get::<_, String>(13)?).unwrap_or_default(),
                    media: serde_json::from_str(&row.get::<_, String>(14)?).unwrap_or_default(),
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(tweets)
    }

    /// Search likes using FTS5.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn search_likes(&self, query: &str, limit: usize) -> Result<Vec<Like>> {
        let limit = limit_to_i64(limit);
        let mut stmt = self.conn.prepare(
            r"
            SELECT l.tweet_id, l.full_text, l.expanded_url
            FROM likes l
            JOIN fts_likes fts ON l.tweet_id = fts.tweet_id
            WHERE fts_likes MATCH ?
            ORDER BY rank
            LIMIT ?
            ",
        )?;

        let likes = stmt
            .query_map(params![query, limit], |row| {
                Ok(Like {
                    tweet_id: row.get(0)?,
                    full_text: row.get(1)?,
                    expanded_url: row.get(2)?,
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(likes)
    }

    /// Search DMs using FTS5.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn search_dms(&self, query: &str, limit: usize) -> Result<Vec<DirectMessage>> {
        let limit = limit_to_i64(limit);
        let mut stmt = self.conn.prepare(
            r"
            SELECT dm.id, dm.conversation_id, dm.sender_id, dm.recipient_id, dm.text,
                   dm.created_at, dm.urls_json, dm.media_urls_json
            FROM direct_messages dm
            JOIN fts_dms fts ON dm.id = fts.dm_id
            WHERE fts_dms MATCH ?
            ORDER BY rank
            LIMIT ?
            ",
        )?;

        let dms = stmt
            .query_map(params![query, limit], |row| {
                Ok(DirectMessage {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    sender_id: row.get(2)?,
                    recipient_id: row.get(3)?,
                    text: row.get(4)?,
                    created_at: parse_rfc3339_or_epoch(row.get::<_, Option<String>>(5)?),
                    urls: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                    media_urls: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(dms)
    }

    /// Get all messages for a conversation, ordered by timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn get_conversation_messages(&self, conversation_id: &str) -> Result<Vec<DirectMessage>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT id, conversation_id, sender_id, recipient_id, text, created_at, urls_json, media_urls_json
            FROM direct_messages
            WHERE conversation_id = ?
            ORDER BY created_at ASC, id ASC
            ",
        )?;

        let messages = stmt
            .query_map(params![conversation_id], |row| {
                Ok(DirectMessage {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    sender_id: row.get(2)?,
                    recipient_id: row.get(3)?,
                    text: row.get(4)?,
                    created_at: parse_rfc3339_or_epoch(row.get::<_, Option<String>>(5)?),
                    urls: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                    media_urls: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(messages)
    }

    /// Search Grok messages using FTS5.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn search_grok(&self, query: &str, limit: usize) -> Result<Vec<GrokMessage>> {
        let limit = limit_to_i64(limit);
        let mut stmt = self.conn.prepare(
            r"
            SELECT g.chat_id, g.message, g.sender, g.created_at, g.grok_mode
            FROM grok_messages g
            WHERE g.rowid IN (
                SELECT rowid FROM fts_grok WHERE fts_grok MATCH ?
            )
            LIMIT ?
            ",
        )?;

        let messages = stmt
            .query_map(params![query, limit], |row| {
                Ok(GrokMessage {
                    chat_id: row.get(0)?,
                    message: row.get(1)?,
                    sender: row.get(2)?,
                    created_at: parse_rfc3339_or_epoch(row.get::<_, Option<String>>(3)?),
                    grok_mode: row.get(4)?,
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(messages)
    }

    /// Get a tweet by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn get_tweet(&self, id: &str) -> Result<Option<Tweet>> {
        let result = self.conn.query_row(
            r"
            SELECT id, created_at, full_text, source, favorite_count, retweet_count,
                   lang, in_reply_to_status_id, in_reply_to_user_id, in_reply_to_screen_name,
                   is_retweet, hashtags_json, mentions_json, urls_json, media_json
            FROM tweets WHERE id = ?
            ",
            params![id],
            |row| {
                Ok(Tweet {
                    id: row.get(0)?,
                    created_at: parse_rfc3339_or_epoch(row.get::<_, Option<String>>(1)?),
                    full_text: row.get(2)?,
                    source: row.get(3)?,
                    favorite_count: row.get(4)?,
                    retweet_count: row.get(5)?,
                    lang: row.get(6)?,
                    in_reply_to_status_id: row.get(7)?,
                    in_reply_to_user_id: row.get(8)?,
                    in_reply_to_screen_name: row.get(9)?,
                    is_retweet: row.get::<_, i32>(10)? != 0,
                    hashtags: serde_json::from_str(&row.get::<_, String>(11)?).unwrap_or_default(),
                    user_mentions: serde_json::from_str(&row.get::<_, String>(12)?)
                        .unwrap_or_default(),
                    urls: serde_json::from_str(&row.get::<_, String>(13)?).unwrap_or_default(),
                    media: serde_json::from_str(&row.get::<_, String>(14)?).unwrap_or_default(),
                })
            },
        );

        match result {
            Ok(tweet) => Ok(Some(tweet)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get replies to a tweet by parent ID, ordered by creation time.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn get_tweet_replies(&self, parent_id: &str) -> Result<Vec<Tweet>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT id, created_at, full_text, source, favorite_count, retweet_count,
                   lang, in_reply_to_status_id, in_reply_to_user_id, in_reply_to_screen_name,
                   is_retweet, hashtags_json, mentions_json, urls_json, media_json
            FROM tweets
            WHERE in_reply_to_status_id = ?
            ORDER BY created_at ASC
            ",
        )?;

        let tweets = stmt
            .query_map(params![parent_id], |row| {
                Ok(Tweet {
                    id: row.get(0)?,
                    created_at: parse_rfc3339_or_epoch(row.get::<_, Option<String>>(1)?),
                    full_text: row.get(2)?,
                    source: row.get(3)?,
                    favorite_count: row.get(4)?,
                    retweet_count: row.get(5)?,
                    lang: row.get(6)?,
                    in_reply_to_status_id: row.get(7)?,
                    in_reply_to_user_id: row.get(8)?,
                    in_reply_to_screen_name: row.get(9)?,
                    is_retweet: row.get::<_, i32>(10)? != 0,
                    hashtags: serde_json::from_str(&row.get::<_, String>(11)?).unwrap_or_default(),
                    user_mentions: serde_json::from_str(&row.get::<_, String>(12)?)
                        .unwrap_or_default(),
                    urls: serde_json::from_str(&row.get::<_, String>(13)?).unwrap_or_default(),
                    media: serde_json::from_str(&row.get::<_, String>(14)?).unwrap_or_default(),
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(tweets)
    }

    /// Get a tweet thread rooted at the earliest ancestor, including all replies.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn get_tweet_thread(&self, id: &str) -> Result<Vec<Tweet>> {
        let Some(mut root) = self.get_tweet(id)? else {
            return Ok(Vec::new());
        };

        let mut seen = HashSet::new();
        seen.insert(root.id.clone());

        while let Some(parent_id) = root.in_reply_to_status_id.clone() {
            if !seen.insert(parent_id.clone()) {
                break;
            }
            match self.get_tweet(&parent_id)? {
                Some(parent) => root = parent,
                None => break,
            }
        }

        let mut thread = Vec::new();
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();

        queue.push_back(root);

        while let Some(tweet) = queue.pop_front() {
            if !visited.insert(tweet.id.clone()) {
                continue;
            }
            let replies = self.get_tweet_replies(&tweet.id)?;
            for reply in replies {
                if !visited.contains(&reply.id) {
                    queue.push_back(reply);
                }
            }
            thread.push(tweet);
        }

        thread.sort_by_key(|tweet| tweet.created_at);
        Ok(thread)
    }

    /// Get all tweets, optionally limited.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn get_all_tweets(&self, limit: Option<usize>) -> Result<Vec<Tweet>> {
        let query = limit.map_or_else(
            || {
                r"SELECT id, created_at, full_text, source, favorite_count, retweet_count,
                   lang, in_reply_to_status_id, in_reply_to_user_id, in_reply_to_screen_name,
                   is_retweet, hashtags_json, mentions_json, urls_json, media_json
                FROM tweets ORDER BY created_at DESC"
                    .to_string()
            },
            |lim| {
                format!(
                    r"SELECT id, created_at, full_text, source, favorite_count, retweet_count,
                   lang, in_reply_to_status_id, in_reply_to_user_id, in_reply_to_screen_name,
                   is_retweet, hashtags_json, mentions_json, urls_json, media_json
                FROM tweets ORDER BY created_at DESC LIMIT {lim}"
                )
            },
        );

        let mut stmt = self.conn.prepare(&query)?;
        let tweets = stmt
            .query_map([], |row| {
                Ok(Tweet {
                    id: row.get(0)?,
                    created_at: parse_rfc3339_or_epoch(row.get::<_, Option<String>>(1)?),
                    full_text: row.get(2)?,
                    source: row.get(3)?,
                    favorite_count: row.get(4)?,
                    retweet_count: row.get(5)?,
                    lang: row.get(6)?,
                    in_reply_to_status_id: row.get(7)?,
                    in_reply_to_user_id: row.get(8)?,
                    in_reply_to_screen_name: row.get(9)?,
                    is_retweet: row.get::<_, i32>(10)? != 0,
                    hashtags: serde_json::from_str(&row.get::<_, String>(11)?).unwrap_or_default(),
                    user_mentions: serde_json::from_str(&row.get::<_, String>(12)?)
                        .unwrap_or_default(),
                    urls: serde_json::from_str(&row.get::<_, String>(13)?).unwrap_or_default(),
                    media: serde_json::from_str(&row.get::<_, String>(14)?).unwrap_or_default(),
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(tweets)
    }

    /// Get all likes, optionally limited.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn get_all_likes(&self, limit: Option<usize>) -> Result<Vec<Like>> {
        let query = limit.map_or_else(
            || "SELECT tweet_id, full_text, expanded_url FROM likes".to_string(),
            |lim| format!("SELECT tweet_id, full_text, expanded_url FROM likes LIMIT {lim}"),
        );

        let mut stmt = self.conn.prepare(&query)?;
        let likes = stmt
            .query_map([], |row| {
                Ok(Like {
                    tweet_id: row.get(0)?,
                    full_text: row.get(1)?,
                    expanded_url: row.get(2)?,
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(likes)
    }

    /// Get all DM conversations with messages, optionally limited.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn get_all_dms(&self, limit: Option<usize>) -> Result<Vec<DirectMessage>> {
        let query = limit.map_or_else(
            || {
                r"SELECT id, conversation_id, sender_id, recipient_id, text,
                   created_at, urls_json, media_urls_json
                FROM direct_messages ORDER BY created_at DESC"
                    .to_string()
            },
            |lim| {
                format!(
                    r"SELECT id, conversation_id, sender_id, recipient_id, text,
                   created_at, urls_json, media_urls_json
                FROM direct_messages ORDER BY created_at DESC LIMIT {lim}"
                )
            },
        );

        let mut stmt = self.conn.prepare(&query)?;
        let dms = stmt
            .query_map([], |row| {
                Ok(DirectMessage {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    sender_id: row.get(2)?,
                    recipient_id: row.get(3)?,
                    text: row.get(4)?,
                    created_at: parse_rfc3339_or_epoch(row.get::<_, Option<String>>(5)?),
                    urls: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                    media_urls: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(dms)
    }

    /// Get DM conversation summaries, optionally limited.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn get_dm_conversation_summaries(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<DmConversationSummary>> {
        let query = limit.map_or_else(
            || {
                r"SELECT conversation_id, participant_ids, message_count,
                   first_message_at, last_message_at
                FROM dm_conversations
                ORDER BY last_message_at DESC"
                    .to_string()
            },
            |lim| {
                format!(
                    r"SELECT conversation_id, participant_ids, message_count,
                   first_message_at, last_message_at
                FROM dm_conversations
                ORDER BY last_message_at DESC LIMIT {lim}"
                )
            },
        );

        let mut stmt = self.conn.prepare(&query)?;
        let summaries = stmt
            .query_map([], |row| {
                let participants: String = row.get(1)?;
                let participant_ids = participants
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();

                Ok(DmConversationSummary {
                    conversation_id: row.get(0)?,
                    participant_ids,
                    message_count: row.get(2)?,
                    first_message_at: parse_rfc3339_opt(row.get::<_, Option<String>>(3)?),
                    last_message_at: parse_rfc3339_opt(row.get::<_, Option<String>>(4)?),
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(summaries)
    }

    /// Get all followers, optionally limited.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn get_all_followers(&self, limit: Option<usize>) -> Result<Vec<Follower>> {
        let query = limit.map_or_else(
            || "SELECT account_id, user_link FROM followers".to_string(),
            |lim| format!("SELECT account_id, user_link FROM followers LIMIT {lim}"),
        );

        let mut stmt = self.conn.prepare(&query)?;
        let followers = stmt
            .query_map([], |row| {
                Ok(Follower {
                    account_id: row.get(0)?,
                    user_link: row.get(1)?,
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(followers)
    }

    /// Get all following, optionally limited.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn get_all_following(&self, limit: Option<usize>) -> Result<Vec<Following>> {
        let query = limit.map_or_else(
            || "SELECT account_id, user_link FROM following".to_string(),
            |lim| format!("SELECT account_id, user_link FROM following LIMIT {lim}"),
        );

        let mut stmt = self.conn.prepare(&query)?;
        let following = stmt
            .query_map([], |row| {
                Ok(Following {
                    account_id: row.get(0)?,
                    user_link: row.get(1)?,
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(following)
    }

    /// Get all blocks, optionally limited.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn get_all_blocks(&self, limit: Option<usize>) -> Result<Vec<Block>> {
        let query = limit.map_or_else(
            || "SELECT account_id, user_link FROM blocks".to_string(),
            |lim| format!("SELECT account_id, user_link FROM blocks LIMIT {lim}"),
        );

        let mut stmt = self.conn.prepare(&query)?;
        let blocks = stmt
            .query_map([], |row| {
                Ok(Block {
                    account_id: row.get(0)?,
                    user_link: row.get(1)?,
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(blocks)
    }

    /// Get all mutes, optionally limited.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn get_all_mutes(&self, limit: Option<usize>) -> Result<Vec<Mute>> {
        let query = limit.map_or_else(
            || "SELECT account_id, user_link FROM mutes".to_string(),
            |lim| format!("SELECT account_id, user_link FROM mutes LIMIT {lim}"),
        );

        let mut stmt = self.conn.prepare(&query)?;
        let mutes = stmt
            .query_map([], |row| {
                Ok(Mute {
                    account_id: row.get(0)?,
                    user_link: row.get(1)?,
                })
            })?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(mutes)
    }
}

fn format_table_stats(stats: &[TableStat]) -> String {
    if stats.is_empty() {
        return "no tables found".to_string();
    }

    stats
        .iter()
        .map(|stat| {
            let size = stat
                .bytes
                .map_or_else(|| "size unavailable".to_string(), format_bytes);
            format!("{}: {} rows ({size})", stat.name, stat.rows)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_bytes(bytes: i64) -> String {
    let bytes = u64::try_from(bytes.max(0)).unwrap_or(0);

    if bytes < BYTES_PER_KB {
        format!("{bytes} B")
    } else if bytes < BYTES_PER_MB {
        format_bytes_with_unit(bytes, BYTES_PER_KB, "KB")
    } else if bytes < BYTES_PER_GB {
        format_bytes_with_unit(bytes, BYTES_PER_MB, "MB")
    } else {
        format_bytes_with_unit(bytes, BYTES_PER_GB, "GB")
    }
}

fn format_bytes_with_unit(bytes: u64, unit: u64, suffix: &str) -> String {
    let whole = bytes / unit;
    let tenths = (bytes % unit) * 10 / unit;
    format!("{whole}.{tenths} {suffix}")
}

fn limit_to_i64(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TweetUrl;
    use chrono::Duration;

    fn create_test_tweet(id: &str, text: &str) -> Tweet {
        Tweet {
            id: id.to_string(),
            created_at: Utc::now(),
            full_text: text.to_string(),
            source: Some("test".to_string()),
            favorite_count: 10,
            retweet_count: 5,
            lang: Some("en".to_string()),
            in_reply_to_status_id: None,
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            is_retweet: false,
            hashtags: vec!["rust".to_string()],
            user_mentions: vec![],
            urls: vec![],
            media: vec![],
        }
    }

    fn create_test_like(tweet_id: &str, text: Option<&str>) -> Like {
        Like {
            tweet_id: tweet_id.to_string(),
            full_text: text.map(str::to_string),
            expanded_url: Some("https://x.com/status/123".to_string()),
        }
    }

    fn create_test_dm(id: &str, text: &str) -> DirectMessage {
        DirectMessage {
            id: id.to_string(),
            conversation_id: "test_conv".to_string(),
            sender_id: "user1".to_string(),
            recipient_id: "user2".to_string(),
            text: text.to_string(),
            created_at: Utc::now(),
            urls: vec![],
            media_urls: vec![],
        }
    }

    fn create_test_grok_message(chat_id: &str, message: &str) -> GrokMessage {
        GrokMessage {
            chat_id: chat_id.to_string(),
            message: message.to_string(),
            sender: "user".to_string(),
            created_at: Utc::now(),
            grok_mode: Some("fun".to_string()),
        }
    }

    #[test]
    fn test_create_database() {
        let storage = Storage::open_memory().unwrap();
        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.tweets_count, 0);
    }

    #[test]
    fn test_store_and_retrieve_tweets() {
        let mut storage = Storage::open_memory().unwrap();

        let tweets = vec![
            create_test_tweet("1", "First tweet about Rust"),
            create_test_tweet("2", "Second tweet about programming"),
        ];

        let count = storage.store_tweets(&tweets).unwrap();
        assert_eq!(count, 2);

        // Verify count
        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.tweets_count, 2);

        // Retrieve by ID
        let tweet = storage.get_tweet("1").unwrap();
        assert!(tweet.is_some());
        let tweet = tweet.unwrap();
        assert_eq!(tweet.id, "1");
        assert_eq!(tweet.full_text, "First tweet about Rust");
    }

    #[test]
    fn test_get_tweet_not_found() {
        let storage = Storage::open_memory().unwrap();
        let tweet = storage.get_tweet("nonexistent").unwrap();
        assert!(tweet.is_none());
    }

    #[test]
    fn test_get_tweet_thread() {
        let mut storage = Storage::open_memory().unwrap();

        let root_date = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let reply_first_date = DateTime::parse_from_rfc3339("2024-01-01T00:01:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let reply_followup_date = DateTime::parse_from_rfc3339("2024-01-01T00:02:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let branch_date = DateTime::parse_from_rfc3339("2024-01-01T00:03:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let root = Tweet {
            id: "1".to_string(),
            created_at: root_date,
            full_text: "Root tweet".to_string(),
            source: None,
            favorite_count: 0,
            retweet_count: 0,
            lang: None,
            in_reply_to_status_id: None,
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            is_retweet: false,
            hashtags: vec![],
            user_mentions: vec![],
            urls: vec![],
            media: vec![],
        };
        let reply = Tweet {
            id: "2".to_string(),
            created_at: reply_first_date,
            full_text: "Reply tweet".to_string(),
            source: None,
            favorite_count: 0,
            retweet_count: 0,
            lang: None,
            in_reply_to_status_id: Some("1".to_string()),
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            is_retweet: false,
            hashtags: vec![],
            user_mentions: vec![],
            urls: vec![],
            media: vec![],
        };
        let reply2 = Tweet {
            id: "3".to_string(),
            created_at: reply_followup_date,
            full_text: "Reply to reply".to_string(),
            source: None,
            favorite_count: 0,
            retweet_count: 0,
            lang: None,
            in_reply_to_status_id: Some("2".to_string()),
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            is_retweet: false,
            hashtags: vec![],
            user_mentions: vec![],
            urls: vec![],
            media: vec![],
        };
        let branch = Tweet {
            id: "4".to_string(),
            created_at: branch_date,
            full_text: "Branch reply".to_string(),
            source: None,
            favorite_count: 0,
            retweet_count: 0,
            lang: None,
            in_reply_to_status_id: Some("1".to_string()),
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            is_retweet: false,
            hashtags: vec![],
            user_mentions: vec![],
            urls: vec![],
            media: vec![],
        };

        storage
            .store_tweets(&[root, reply, reply2, branch])
            .unwrap();

        let thread = storage.get_tweet_thread("3").unwrap();
        assert_eq!(thread.len(), 4);
        assert_eq!(thread[0].id, "1");
        assert_eq!(thread[1].id, "2");
        assert_eq!(thread[2].id, "3");
        assert_eq!(thread[3].id, "4");
    }

    #[test]
    fn test_store_and_search_tweets_fts() {
        let mut storage = Storage::open_memory().unwrap();

        let tweets = vec![
            create_test_tweet("1", "Rust programming is awesome"),
            create_test_tweet("2", "Python programming is also good"),
            create_test_tweet("3", "Hello world example"),
        ];

        storage.store_tweets(&tweets).unwrap();

        // Search for "programming"
        let results = storage.search_tweets("programming", 10).unwrap();
        assert_eq!(results.len(), 2);

        // Search for "rust"
        let results = storage.search_tweets("rust", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "1");
    }

    #[test]
    fn test_store_likes() {
        let mut storage = Storage::open_memory().unwrap();

        let likes = vec![
            create_test_like("like1", Some("Great content")),
            create_test_like("like2", Some("Another liked tweet")),
            create_test_like("like3", None),
        ];

        let count = storage.store_likes(&likes).unwrap();
        assert_eq!(count, 3);

        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.likes_count, 3);
    }

    #[test]
    fn test_search_likes_fts() {
        let mut storage = Storage::open_memory().unwrap();

        let likes = vec![
            create_test_like("like1", Some("Rust programming content")),
            create_test_like("like2", Some("Python content")),
        ];

        storage.store_likes(&likes).unwrap();

        let results = storage.search_likes("rust", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tweet_id, "like1");
    }

    #[test]
    fn test_store_dm_conversations() {
        let mut storage = Storage::open_memory().unwrap();

        let conversations = vec![
            DmConversation {
                conversation_id: "conv1".to_string(),
                messages: vec![
                    create_test_dm("dm1", "Hello!"),
                    create_test_dm("dm2", "Hi there!"),
                ],
            },
            DmConversation {
                conversation_id: "conv2".to_string(),
                messages: vec![create_test_dm("dm3", "Another conversation")],
            },
        ];

        let count = storage.store_dm_conversations(&conversations).unwrap();
        assert_eq!(count, 3); // Total messages

        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.dm_conversations_count, 2);
        assert_eq!(stats.dms_count, 3);
    }

    #[test]
    fn test_search_dms_fts() {
        let mut storage = Storage::open_memory().unwrap();

        let conversations = vec![DmConversation {
            conversation_id: "conv1".to_string(),
            messages: vec![
                create_test_dm("dm1", "Let's discuss Rust"),
                create_test_dm("dm2", "What about Python?"),
            ],
        }];

        storage.store_dm_conversations(&conversations).unwrap();

        let results = storage.search_dms("rust", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "dm1");
    }

    #[test]
    fn test_get_conversation_messages() {
        let mut storage = Storage::open_memory().unwrap();

        let base_time = DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let later_time = base_time + Duration::minutes(5);

        let conversation = DmConversation {
            conversation_id: "conv1".to_string(),
            messages: vec![
                DirectMessage {
                    id: "dm2".to_string(),
                    conversation_id: "conv1".to_string(),
                    sender_id: "user2".to_string(),
                    recipient_id: "user1".to_string(),
                    text: "Second message".to_string(),
                    created_at: later_time,
                    urls: vec![],
                    media_urls: vec![],
                },
                DirectMessage {
                    id: "dm1".to_string(),
                    conversation_id: "conv1".to_string(),
                    sender_id: "user1".to_string(),
                    recipient_id: "user2".to_string(),
                    text: "First message".to_string(),
                    created_at: base_time,
                    urls: vec![],
                    media_urls: vec![],
                },
            ],
        };

        storage.store_dm_conversations(&[conversation]).unwrap();

        let messages = storage.get_conversation_messages("conv1").unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].id, "dm1");
        assert_eq!(messages[1].id, "dm2");
    }

    #[test]
    fn test_get_dm_conversation_summaries() {
        let mut storage = Storage::open_memory().unwrap();

        let base_time = DateTime::parse_from_rfc3339("2024-02-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let later_time = base_time + Duration::minutes(30);

        let conversation = DmConversation {
            conversation_id: "conv_summary".to_string(),
            messages: vec![
                DirectMessage {
                    id: "dm1".to_string(),
                    conversation_id: "conv_summary".to_string(),
                    sender_id: "user2".to_string(),
                    recipient_id: "user1".to_string(),
                    text: "Second message".to_string(),
                    created_at: later_time,
                    urls: vec![],
                    media_urls: vec![],
                },
                DirectMessage {
                    id: "dm0".to_string(),
                    conversation_id: "conv_summary".to_string(),
                    sender_id: "user1".to_string(),
                    recipient_id: "user2".to_string(),
                    text: "First message".to_string(),
                    created_at: base_time,
                    urls: vec![],
                    media_urls: vec![],
                },
            ],
        };

        storage.store_dm_conversations(&[conversation]).unwrap();

        let summaries = storage.get_dm_conversation_summaries(None).unwrap();
        assert_eq!(summaries.len(), 1);

        let summary = &summaries[0];
        assert_eq!(summary.conversation_id, "conv_summary");
        assert_eq!(summary.message_count, 2);
        assert_eq!(summary.participant_ids, vec!["user1", "user2"]);
        assert_eq!(summary.first_message_at, Some(base_time));
        assert_eq!(summary.last_message_at, Some(later_time));
    }

    #[test]
    fn test_get_conversation_messages_empty() {
        // Empty/missing conversation_id should return empty vec, not error
        let storage = Storage::open_memory().unwrap();
        let messages = storage.get_conversation_messages("nonexistent").unwrap();
        assert!(messages.is_empty());
    }

    #[test]
    fn test_get_conversation_messages_single() {
        let mut storage = Storage::open_memory().unwrap();

        let conversation = DmConversation {
            conversation_id: "single_conv".to_string(),
            messages: vec![DirectMessage {
                id: "dm_only".to_string(),
                conversation_id: "single_conv".to_string(),
                sender_id: "alice".to_string(),
                recipient_id: "bob".to_string(),
                text: "Single message".to_string(),
                created_at: DateTime::parse_from_rfc3339("2024-06-15T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                urls: vec![],
                media_urls: vec![],
            }],
        };

        storage.store_dm_conversations(&[conversation]).unwrap();

        let messages = storage.get_conversation_messages("single_conv").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, "dm_only");
        assert_eq!(messages[0].sender_id, "alice");
        assert_eq!(messages[0].text, "Single message");
    }

    #[test]
    fn test_get_conversation_messages_field_preservation() {
        // Verify URLs and media_urls round-trip correctly
        let mut storage = Storage::open_memory().unwrap();

        let conversation = DmConversation {
            conversation_id: "rich_conv".to_string(),
            messages: vec![DirectMessage {
                id: "dm_rich".to_string(),
                conversation_id: "rich_conv".to_string(),
                sender_id: "user1".to_string(),
                recipient_id: "user2".to_string(),
                text: "Check out this link!".to_string(),
                created_at: DateTime::parse_from_rfc3339("2024-07-01T09:30:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                urls: vec![TweetUrl {
                    url: "https://t.co/abc".to_string(),
                    expanded_url: Some("https://example.com/article".to_string()),
                    display_url: Some("example.com/article".to_string()),
                }],
                media_urls: vec![
                    "https://pbs.twimg.com/media/abc.jpg".to_string(),
                    "https://pbs.twimg.com/media/def.png".to_string(),
                ],
            }],
        };

        storage.store_dm_conversations(&[conversation]).unwrap();

        let messages = storage.get_conversation_messages("rich_conv").unwrap();
        assert_eq!(messages.len(), 1);

        let msg = &messages[0];
        assert_eq!(msg.urls.len(), 1);
        assert_eq!(msg.urls[0].url, "https://t.co/abc");
        assert_eq!(
            msg.urls[0].expanded_url,
            Some("https://example.com/article".to_string())
        );

        assert_eq!(msg.media_urls.len(), 2);
        assert_eq!(msg.media_urls[0], "https://pbs.twimg.com/media/abc.jpg");
        assert_eq!(msg.media_urls[1], "https://pbs.twimg.com/media/def.png");
    }

    #[test]
    fn test_get_conversation_messages_id_tiebreaker() {
        // When timestamps are identical, id should be used as tiebreaker
        let mut storage = Storage::open_memory().unwrap();

        let same_time = DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let conversation = DmConversation {
            conversation_id: "tie_conv".to_string(),
            messages: vec![
                DirectMessage {
                    id: "dm_z".to_string(),
                    conversation_id: "tie_conv".to_string(),
                    sender_id: "user1".to_string(),
                    recipient_id: "user2".to_string(),
                    text: "Message Z".to_string(),
                    created_at: same_time,
                    urls: vec![],
                    media_urls: vec![],
                },
                DirectMessage {
                    id: "dm_a".to_string(),
                    conversation_id: "tie_conv".to_string(),
                    sender_id: "user2".to_string(),
                    recipient_id: "user1".to_string(),
                    text: "Message A".to_string(),
                    created_at: same_time,
                    urls: vec![],
                    media_urls: vec![],
                },
            ],
        };

        storage.store_dm_conversations(&[conversation]).unwrap();

        let messages = storage.get_conversation_messages("tie_conv").unwrap();
        assert_eq!(messages.len(), 2);
        // dm_a should come before dm_z (alphabetical order)
        assert_eq!(messages[0].id, "dm_a");
        assert_eq!(messages[1].id, "dm_z");
    }

    #[test]
    fn test_store_followers() {
        let mut storage = Storage::open_memory().unwrap();

        let followers = vec![
            Follower {
                account_id: "123".to_string(),
                user_link: Some("https://x.com/user123".to_string()),
            },
            Follower {
                account_id: "456".to_string(),
                user_link: None,
            },
        ];

        let count = storage.store_followers(&followers).unwrap();
        assert_eq!(count, 2);

        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.followers_count, 2);
    }

    #[test]
    fn test_store_following() {
        let mut storage = Storage::open_memory().unwrap();

        let following = vec![Following {
            account_id: "789".to_string(),
            user_link: Some("https://x.com/user789".to_string()),
        }];

        let count = storage.store_following(&following).unwrap();
        assert_eq!(count, 1);

        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.following_count, 1);
    }

    #[test]
    fn test_store_blocks() {
        let mut storage = Storage::open_memory().unwrap();

        let blocks = vec![Block {
            account_id: "blocked1".to_string(),
            user_link: None,
        }];
        let count = storage.store_blocks(&blocks).unwrap();
        assert_eq!(count, 1);

        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.blocks_count, 1);
    }

    #[test]
    fn test_store_mutes() {
        let mut storage = Storage::open_memory().unwrap();

        let mutes = vec![
            Mute {
                account_id: "muted1".to_string(),
                user_link: None,
            },
            Mute {
                account_id: "muted2".to_string(),
                user_link: Some("https://x.com/muted2".to_string()),
            },
        ];

        let count = storage.store_mutes(&mutes).unwrap();
        assert_eq!(count, 2);

        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.mutes_count, 2);
    }

    #[test]
    fn test_store_grok_messages() {
        let mut storage = Storage::open_memory().unwrap();

        let messages = vec![
            create_test_grok_message("chat1", "What is AI?"),
            create_test_grok_message("chat1", "AI is artificial intelligence"),
            create_test_grok_message("chat2", "Different topic"),
        ];

        let count = storage.store_grok_messages(&messages).unwrap();
        assert_eq!(count, 3);

        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.grok_messages_count, 3);
    }

    #[test]
    fn test_search_grok_fts() {
        let mut storage = Storage::open_memory().unwrap();

        let messages = vec![
            create_test_grok_message("chat1", "Machine learning algorithms"),
            create_test_grok_message("chat2", "Web development basics"),
        ];

        storage.store_grok_messages(&messages).unwrap();

        let results = storage.search_grok("machine", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].message.contains("Machine learning"));
    }

    #[test]
    fn test_store_archive_info() {
        let storage = Storage::open_memory().unwrap();

        let info = ArchiveInfo {
            account_id: "12345".to_string(),
            username: "testuser".to_string(),
            display_name: Some("Test User".to_string()),
            archive_size_bytes: 1_024_000,
            generation_date: Utc::now(),
            is_partial: false,
        };

        storage.store_archive_info(&info).unwrap();
        // No error means success
    }

    #[test]
    fn test_get_stats_with_data() {
        let mut storage = Storage::open_memory().unwrap();

        // Store various data types
        storage
            .store_tweets(&[create_test_tweet("1", "Tweet")])
            .unwrap();
        storage
            .store_likes(&[create_test_like("l1", Some("Like"))])
            .unwrap();
        storage
            .store_followers(&[Follower {
                account_id: "f1".to_string(),
                user_link: None,
            }])
            .unwrap();
        storage
            .store_following(&[Following {
                account_id: "fo1".to_string(),
                user_link: None,
            }])
            .unwrap();
        storage
            .store_blocks(&[Block {
                account_id: "b1".to_string(),
                user_link: None,
            }])
            .unwrap();
        storage
            .store_mutes(&[Mute {
                account_id: "m1".to_string(),
                user_link: None,
            }])
            .unwrap();
        storage
            .store_grok_messages(&[create_test_grok_message("c1", "Grok")])
            .unwrap();

        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.tweets_count, 1);
        assert_eq!(stats.likes_count, 1);
        assert_eq!(stats.followers_count, 1);
        assert_eq!(stats.following_count, 1);
        assert_eq!(stats.blocks_count, 1);
        assert_eq!(stats.mutes_count, 1);
        assert_eq!(stats.grok_messages_count, 1);
    }

    #[test]
    fn test_tweet_upsert() {
        let mut storage = Storage::open_memory().unwrap();

        // Store initial tweet
        storage
            .store_tweets(&[create_test_tweet("1", "Original text")])
            .unwrap();

        // Store updated tweet with same ID
        let updated = Tweet {
            id: "1".to_string(),
            created_at: Utc::now(),
            full_text: "Updated text".to_string(),
            source: Some("test".to_string()),
            favorite_count: 100, // Changed
            retweet_count: 50,   // Changed
            lang: Some("en".to_string()),
            in_reply_to_status_id: None,
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            is_retweet: false,
            hashtags: vec![],
            user_mentions: vec![],
            urls: vec![],
            media: vec![],
        };
        storage.store_tweets(&[updated]).unwrap();

        // Should still have 1 tweet (upsert, not insert)
        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.tweets_count, 1);

        // Verify updated content
        let tweet = storage.get_tweet("1").unwrap().unwrap();
        assert_eq!(tweet.full_text, "Updated text");
        assert_eq!(tweet.favorite_count, 100);
    }

    #[test]
    fn test_search_limit() {
        let mut storage = Storage::open_memory().unwrap();

        // Store many tweets
        let tweets: Vec<Tweet> = (0..20)
            .map(|i| create_test_tweet(&format!("{i}"), "common search term"))
            .collect();
        storage.store_tweets(&tweets).unwrap();

        // Search with limit
        let results = storage.search_tweets("common", 5).unwrap();
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn test_stats_date_range() {
        let mut storage = Storage::open_memory().unwrap();

        let early_date = DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let late_date = DateTime::parse_from_rfc3339("2024-12-31T23:59:59Z")
            .unwrap()
            .with_timezone(&Utc);

        let tweets = vec![
            Tweet {
                id: "1".to_string(),
                created_at: early_date,
                full_text: "Early tweet".to_string(),
                source: None,
                favorite_count: 0,
                retweet_count: 0,
                lang: None,
                in_reply_to_status_id: None,
                in_reply_to_user_id: None,
                in_reply_to_screen_name: None,
                is_retweet: false,
                hashtags: vec![],
                user_mentions: vec![],
                urls: vec![],
                media: vec![],
            },
            Tweet {
                id: "2".to_string(),
                created_at: late_date,
                full_text: "Late tweet".to_string(),
                source: None,
                favorite_count: 0,
                retweet_count: 0,
                lang: None,
                in_reply_to_status_id: None,
                in_reply_to_user_id: None,
                in_reply_to_screen_name: None,
                is_retweet: false,
                hashtags: vec![],
                user_mentions: vec![],
                urls: vec![],
                media: vec![],
            },
        ];

        storage.store_tweets(&tweets).unwrap();

        let stats = storage.get_stats().unwrap();
        assert!(stats.first_tweet_date.is_some());
        assert!(stats.last_tweet_date.is_some());

        let first = stats.first_tweet_date.unwrap();
        let last = stats.last_tweet_date.unwrap();
        assert!(first < last);
    }

    #[test]
    fn test_schema_version() {
        let storage = Storage::open_memory().unwrap();
        let version = storage.get_schema_version();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_database_health_checks_pass() {
        let storage = Storage::open_memory().unwrap();
        let checks = storage.database_health_checks();

        let integrity = checks
            .iter()
            .find(|c| c.name == "PRAGMA integrity_check")
            .expect("integrity check missing");
        assert_eq!(integrity.status, CheckStatus::Pass);

        let schema = checks
            .iter()
            .find(|c| c.name == "Schema version")
            .expect("schema check missing");
        assert_eq!(schema.status, CheckStatus::Pass);
    }

    #[test]
    fn test_database_health_orphaned_fts() {
        let storage = Storage::open_memory().unwrap();

        storage
            .connection()
            .execute(
                "INSERT INTO fts_tweets (tweet_id, full_text) VALUES ('orphan', 'text')",
                [],
            )
            .unwrap();

        let checks = storage.database_health_checks();
        let orphaned = checks
            .iter()
            .find(|c| c.name == "FTS orphaned rows (tweets)")
            .expect("orphan check missing");
        assert_eq!(orphaned.status, CheckStatus::Warning);
    }
}
