//! SQLite storage for X archive data.
//!
//! Provides persistent storage with optimized schema for fast queries.

use crate::model::*;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::path::Path;
use tracing::info;

const SCHEMA_VERSION: i32 = 1;

/// SQLite storage manager
pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// Open or create the database at the given path
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(db_path.as_ref())
            .with_context(|| format!("Failed to open database at {}", db_path.as_ref().display()))?;

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

        let mut storage = Self { conn };
        storage.migrate()?;
        Ok(storage)
    }

    /// Open an in-memory database (for testing)
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA temp_store = MEMORY;
            ",
        )?;
        let mut storage = Self { conn };
        storage.migrate()?;
        Ok(storage)
    }

    /// Run database migrations
    fn migrate(&mut self) -> Result<()> {
        let current_version = self.get_schema_version()?;

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

    fn get_schema_version(&self) -> Result<i32> {
        let result: Result<i32, _> =
            self.conn
                .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |row| {
                    let value: String = row.get(0)?;
                    Ok(value.parse().unwrap_or(0))
                });

        match result {
            Ok(v) => Ok(v),
            Err(_) => Ok(0), // Table doesn't exist yet
        }
    }

    fn set_schema_version(&self, version: i32) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?)",
            params![version.to_string()],
        )?;
        Ok(())
    }

    fn create_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
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
            "#,
        )?;

        Ok(())
    }

    /// Store archive info
    pub fn store_archive_info(&self, info: &ArchiveInfo) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO archive_info
            (id, account_id, username, display_name, archive_size_bytes, generation_date, is_partial, indexed_at)
            VALUES (1, ?, ?, ?, ?, ?, ?, ?)
            "#,
            params![
                info.account_id,
                info.username,
                info.display_name,
                info.archive_size_bytes,
                info.generation_date.to_rfc3339(),
                info.is_partial as i32,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Store tweets in a transaction
    pub fn store_tweets(&mut self, tweets: &[Tweet]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;

        {
            let mut stmt = tx.prepare(
                r#"
                INSERT OR REPLACE INTO tweets
                (id, created_at, full_text, source, favorite_count, retweet_count, lang,
                 in_reply_to_status_id, in_reply_to_user_id, in_reply_to_screen_name,
                 is_retweet, hashtags_json, mentions_json, urls_json, media_json)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )?;

            let mut fts_stmt = tx.prepare(
                "INSERT OR REPLACE INTO fts_tweets (tweet_id, full_text) VALUES (?, ?)",
            )?;

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
                    tweet.is_retweet as i32,
                    serde_json::to_string(&tweet.hashtags)?,
                    serde_json::to_string(&tweet.user_mentions)?,
                    serde_json::to_string(&tweet.urls)?,
                    serde_json::to_string(&tweet.media)?,
                ])?;
                fts_stmt.execute(params![tweet.id, tweet.full_text])?;
                count += 1;
            }
        }

        tx.commit()?;
        info!("Stored {} tweets", count);
        Ok(count)
    }

    /// Store likes in a transaction
    pub fn store_likes(&mut self, likes: &[Like]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;

        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO likes (tweet_id, full_text, expanded_url) VALUES (?, ?, ?)",
            )?;
            let mut fts_stmt = tx.prepare(
                "INSERT OR REPLACE INTO fts_likes (tweet_id, full_text) VALUES (?, ?)",
            )?;

            for like in likes {
                stmt.execute(params![like.tweet_id, like.full_text, like.expanded_url])?;
                if let Some(text) = &like.full_text {
                    fts_stmt.execute(params![like.tweet_id, text])?;
                }
                count += 1;
            }
        }

        tx.commit()?;
        info!("Stored {} likes", count);
        Ok(count)
    }

    /// Store DM conversations and messages
    pub fn store_dm_conversations(&mut self, conversations: &[DmConversation]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut message_count = 0;

        {
            let mut conv_stmt = tx.prepare(
                r#"
                INSERT OR REPLACE INTO dm_conversations
                (conversation_id, participant_ids, message_count, first_message_at, last_message_at)
                VALUES (?, ?, ?, ?, ?)
                "#,
            )?;

            let mut msg_stmt = tx.prepare(
                r#"
                INSERT OR REPLACE INTO direct_messages
                (id, conversation_id, sender_id, recipient_id, text, created_at, urls_json, media_urls_json)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )?;

            let mut fts_stmt = tx.prepare(
                "INSERT OR REPLACE INTO fts_dms (dm_id, text) VALUES (?, ?)",
            )?;

            for conv in conversations {
                // Get participant IDs and date range
                let mut participant_ids: Vec<&str> = conv
                    .messages
                    .iter()
                    .flat_map(|m| vec![m.sender_id.as_str(), m.recipient_id.as_str()])
                    .collect();
                participant_ids.sort();
                participant_ids.dedup();

                let first_msg = conv.messages.iter().min_by_key(|m| m.created_at);
                let last_msg = conv.messages.iter().max_by_key(|m| m.created_at);

                conv_stmt.execute(params![
                    conv.conversation_id,
                    participant_ids.join(","),
                    conv.messages.len() as i64,
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
                    fts_stmt.execute(params![msg.id, msg.text])?;
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

    /// Store followers
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

    /// Store following
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

    /// Store blocks
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

    /// Store mutes
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

    /// Store Grok messages
    pub fn store_grok_messages(&mut self, messages: &[GrokMessage]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut count = 0;

        {
            let mut stmt = tx.prepare(
                r#"
                INSERT INTO grok_messages (chat_id, message, sender, created_at, grok_mode)
                VALUES (?, ?, ?, ?, ?)
                "#,
            )?;

            let mut fts_stmt = tx.prepare(
                "INSERT INTO fts_grok (grok_id, message) VALUES (?, ?)",
            )?;

            for msg in messages {
                stmt.execute(params![
                    msg.chat_id,
                    msg.message,
                    msg.sender,
                    msg.created_at.to_rfc3339(),
                    msg.grok_mode,
                ])?;
                // Use chat_id + timestamp as unique ID for FTS
                let grok_id = format!("{}_{}", msg.chat_id, msg.created_at.timestamp());
                fts_stmt.execute(params![grok_id, msg.message])?;
                count += 1;
            }
        }

        tx.commit()?;
        info!("Stored {} Grok messages", count);
        Ok(count)
    }

    /// Get archive statistics
    pub fn get_stats(&self) -> Result<ArchiveStats> {
        let tweets_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM tweets", [], |row| row.get(0))?;

        let likes_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM likes", [], |row| row.get(0))?;

        let dms_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM direct_messages", [], |row| row.get(0))?;

        let dm_conversations_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM dm_conversations", [], |row| {
                row.get(0)
            })?;

        let followers_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM followers", [], |row| row.get(0))?;

        let following_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM following", [], |row| row.get(0))?;

        let blocks_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM blocks", [], |row| row.get(0))?;

        let mutes_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM mutes", [], |row| row.get(0))?;

        let grok_messages_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM grok_messages", [], |row| row.get(0))?;

        let first_tweet_date: Option<String> = self
            .conn
            .query_row(
                "SELECT MIN(created_at) FROM tweets",
                [],
                |row| row.get(0),
            )
            .ok();

        let last_tweet_date: Option<String> = self
            .conn
            .query_row(
                "SELECT MAX(created_at) FROM tweets",
                [],
                |row| row.get(0),
            )
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

    /// Search tweets using FTS5
    pub fn search_tweets(&self, query: &str, limit: usize) -> Result<Vec<Tweet>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT t.id, t.created_at, t.full_text, t.source, t.favorite_count, t.retweet_count,
                   t.lang, t.in_reply_to_status_id, t.in_reply_to_user_id, t.in_reply_to_screen_name,
                   t.is_retweet, t.hashtags_json, t.mentions_json, t.urls_json, t.media_json
            FROM tweets t
            JOIN fts_tweets fts ON t.id = fts.tweet_id
            WHERE fts_tweets MATCH ?
            ORDER BY rank
            LIMIT ?
            "#,
        )?;

        let tweets = stmt
            .query_map(params![query, limit as i64], |row| {
                Ok(Tweet {
                    id: row.get(0)?,
                    created_at: row
                        .get::<_, String>(1)
                        .ok()
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now),
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
            .filter_map(|r| r.ok())
            .collect();

        Ok(tweets)
    }

    /// Search likes using FTS5
    pub fn search_likes(&self, query: &str, limit: usize) -> Result<Vec<Like>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT l.tweet_id, l.full_text, l.expanded_url
            FROM likes l
            JOIN fts_likes fts ON l.tweet_id = fts.tweet_id
            WHERE fts_likes MATCH ?
            ORDER BY rank
            LIMIT ?
            "#,
        )?;

        let likes = stmt
            .query_map(params![query, limit as i64], |row| {
                Ok(Like {
                    tweet_id: row.get(0)?,
                    full_text: row.get(1)?,
                    expanded_url: row.get(2)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(likes)
    }

    /// Search DMs using FTS5
    pub fn search_dms(&self, query: &str, limit: usize) -> Result<Vec<DirectMessage>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT dm.id, dm.conversation_id, dm.sender_id, dm.recipient_id, dm.text,
                   dm.created_at, dm.urls_json, dm.media_urls_json
            FROM direct_messages dm
            JOIN fts_dms fts ON dm.id = fts.dm_id
            WHERE fts_dms MATCH ?
            ORDER BY rank
            LIMIT ?
            "#,
        )?;

        let dms = stmt
            .query_map(params![query, limit as i64], |row| {
                Ok(DirectMessage {
                    id: row.get(0)?,
                    sender_id: row.get(2)?,
                    recipient_id: row.get(3)?,
                    text: row.get(4)?,
                    created_at: row
                        .get::<_, String>(5)
                        .ok()
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now),
                    urls: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                    media_urls: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(dms)
    }

    /// Search Grok messages using FTS5
    pub fn search_grok(&self, query: &str, limit: usize) -> Result<Vec<GrokMessage>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT g.chat_id, g.message, g.sender, g.created_at, g.grok_mode
            FROM grok_messages g
            WHERE g.rowid IN (
                SELECT rowid FROM fts_grok WHERE fts_grok MATCH ?
            )
            LIMIT ?
            "#,
        )?;

        let messages = stmt
            .query_map(params![query, limit as i64], |row| {
                Ok(GrokMessage {
                    chat_id: row.get(0)?,
                    message: row.get(1)?,
                    sender: row.get(2)?,
                    created_at: row
                        .get::<_, String>(3)
                        .ok()
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now),
                    grok_mode: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(messages)
    }

    /// Get a tweet by ID
    pub fn get_tweet(&self, id: &str) -> Result<Option<Tweet>> {
        let result = self.conn.query_row(
            r#"
            SELECT id, created_at, full_text, source, favorite_count, retweet_count,
                   lang, in_reply_to_status_id, in_reply_to_user_id, in_reply_to_screen_name,
                   is_retweet, hashtags_json, mentions_json, urls_json, media_json
            FROM tweets WHERE id = ?
            "#,
            params![id],
            |row| {
                Ok(Tweet {
                    id: row.get(0)?,
                    created_at: row
                        .get::<_, String>(1)
                        .ok()
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_database() {
        let storage = Storage::open_memory().unwrap();
        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.tweets_count, 0);
    }
}
