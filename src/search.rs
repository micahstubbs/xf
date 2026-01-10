//! Tantivy-based full-text search engine for X data.
//!
//! Provides ultra-fast search with BM25 ranking, prefix matching, and phrase queries.

use crate::model::*;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, Query, QueryParser, TermQuery};
use tantivy::schema::*;
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};
use tracing::info;

/// Schema field names
const FIELD_ID: &str = "id";
const FIELD_TEXT: &str = "text";
const FIELD_TEXT_PREFIX: &str = "text_prefix";
const FIELD_TYPE: &str = "type";
const FIELD_CREATED_AT: &str = "created_at";
const FIELD_METADATA: &str = "metadata";

/// Document types stored in the index
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocType {
    Tweet,
    Like,
    DirectMessage,
    GrokMessage,
}

impl DocType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Tweet => "tweet",
            Self::Like => "like",
            Self::DirectMessage => "dm",
            Self::GrokMessage => "grok",
        }
    }

    #[allow(dead_code)]
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "tweet" => Some(Self::Tweet),
            "like" => Some(Self::Like),
            "dm" => Some(Self::DirectMessage),
            "grok" => Some(Self::GrokMessage),
            _ => None,
        }
    }
}

/// Build the Tantivy schema
fn build_schema() -> Schema {
    let mut schema_builder = Schema::builder();

    // ID field - stored but not indexed for search
    schema_builder.add_text_field(FIELD_ID, STRING | STORED);

    // Main text field - tokenized for full-text search
    let text_options = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("default")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    schema_builder.add_text_field(FIELD_TEXT, text_options);

    // Prefix text field - for edge n-gram style prefix matching
    let prefix_options = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("raw")
                .set_index_option(IndexRecordOption::Basic),
        );
    schema_builder.add_text_field(FIELD_TEXT_PREFIX, prefix_options);

    // Document type - exact match only
    schema_builder.add_text_field(FIELD_TYPE, STRING | STORED);

    // Created at timestamp - for sorting and range queries
    schema_builder.add_i64_field(FIELD_CREATED_AT, INDEXED | STORED | FAST);

    // Metadata JSON - stored for retrieval
    schema_builder.add_text_field(FIELD_METADATA, STORED);

    schema_builder.build()
}

/// Search engine wrapping Tantivy
pub struct SearchEngine {
    index: Index,
    schema: Schema,
    reader: IndexReader,
}

impl SearchEngine {
    /// Create or open an index at the given path
    pub fn open(index_path: impl AsRef<Path>) -> Result<Self> {
        let index_path = index_path.as_ref();
        std::fs::create_dir_all(index_path)?;

        let schema = build_schema();

        let index = if index_path.join("meta.json").exists() {
            Index::open_in_dir(index_path)
                .with_context(|| format!("Failed to open index at {}", index_path.display()))?
        } else {
            Index::create_in_dir(index_path, schema.clone())
                .with_context(|| format!("Failed to create index at {}", index_path.display()))?
        };

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        Ok(Self {
            index,
            schema,
            reader,
        })
    }

    /// Create an in-memory index (for testing)
    pub fn open_memory() -> Result<Self> {
        let schema = build_schema();
        let index = Index::create_in_ram(schema.clone());

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        Ok(Self {
            index,
            schema,
            reader,
        })
    }

    /// Get a writer for indexing
    pub fn writer(&self, heap_size: usize) -> Result<IndexWriter> {
        self.index
            .writer(heap_size)
            .context("Failed to create index writer")
    }

    /// Reload the reader to see committed changes
    pub fn reload(&self) -> Result<()> {
        self.reader.reload()?;
        Ok(())
    }

    /// Get schema fields
    fn get_fields(&self) -> (Field, Field, Field, Field, Field, Field) {
        (
            self.schema.get_field(FIELD_ID).unwrap(),
            self.schema.get_field(FIELD_TEXT).unwrap(),
            self.schema.get_field(FIELD_TEXT_PREFIX).unwrap(),
            self.schema.get_field(FIELD_TYPE).unwrap(),
            self.schema.get_field(FIELD_CREATED_AT).unwrap(),
            self.schema.get_field(FIELD_METADATA).unwrap(),
        )
    }

    /// Index tweets
    pub fn index_tweets(&self, writer: &mut IndexWriter, tweets: &[Tweet]) -> Result<usize> {
        let (id_field, text_field, prefix_field, type_field, created_at_field, metadata_field) =
            self.get_fields();

        let mut count = 0;
        for tweet in tweets {
            // Generate prefix terms
            let prefixes = generate_prefixes(&tweet.full_text);

            let metadata = serde_json::json!({
                "favorite_count": tweet.favorite_count,
                "retweet_count": tweet.retweet_count,
                "in_reply_to": tweet.in_reply_to_screen_name,
                "hashtags": tweet.hashtags,
                "source": tweet.source,
            });

            writer.add_document(doc!(
                id_field => tweet.id.clone(),
                text_field => tweet.full_text.clone(),
                prefix_field => prefixes,
                type_field => DocType::Tweet.as_str(),
                created_at_field => tweet.created_at.timestamp(),
                metadata_field => metadata.to_string(),
            ))?;
            count += 1;
        }

        info!("Indexed {} tweets", count);
        Ok(count)
    }

    /// Index likes
    pub fn index_likes(&self, writer: &mut IndexWriter, likes: &[Like]) -> Result<usize> {
        let (id_field, text_field, prefix_field, type_field, created_at_field, metadata_field) =
            self.get_fields();

        let mut count = 0;
        for like in likes {
            if let Some(text) = &like.full_text {
                let prefixes = generate_prefixes(text);

                let metadata = serde_json::json!({
                    "expanded_url": like.expanded_url,
                });

                writer.add_document(doc!(
                    id_field => like.tweet_id.clone(),
                    text_field => text.clone(),
                    prefix_field => prefixes,
                    type_field => DocType::Like.as_str(),
                    created_at_field => 0i64, // Likes don't have timestamps
                    metadata_field => metadata.to_string(),
                ))?;
                count += 1;
            }
        }

        info!("Indexed {} likes", count);
        Ok(count)
    }

    /// Index direct messages
    pub fn index_dms(&self, writer: &mut IndexWriter, conversations: &[DmConversation]) -> Result<usize> {
        let (id_field, text_field, prefix_field, type_field, created_at_field, metadata_field) =
            self.get_fields();

        let mut count = 0;
        for conv in conversations {
            for msg in &conv.messages {
                let prefixes = generate_prefixes(&msg.text);

                let metadata = serde_json::json!({
                    "conversation_id": conv.conversation_id,
                    "sender_id": msg.sender_id,
                    "recipient_id": msg.recipient_id,
                });

                writer.add_document(doc!(
                    id_field => msg.id.clone(),
                    text_field => msg.text.clone(),
                    prefix_field => prefixes,
                    type_field => DocType::DirectMessage.as_str(),
                    created_at_field => msg.created_at.timestamp(),
                    metadata_field => metadata.to_string(),
                ))?;
                count += 1;
            }
        }

        info!("Indexed {} DMs", count);
        Ok(count)
    }

    /// Index Grok messages
    pub fn index_grok_messages(&self, writer: &mut IndexWriter, messages: &[GrokMessage]) -> Result<usize> {
        let (id_field, text_field, prefix_field, type_field, created_at_field, metadata_field) =
            self.get_fields();

        let mut count = 0;
        for msg in messages {
            let prefixes = generate_prefixes(&msg.message);

            let metadata = serde_json::json!({
                "chat_id": msg.chat_id,
                "sender": msg.sender,
                "grok_mode": msg.grok_mode,
            });

            // Use chat_id + timestamp as unique ID
            let doc_id = format!("{}_{}", msg.chat_id, msg.created_at.timestamp());

            writer.add_document(doc!(
                id_field => doc_id,
                text_field => msg.message.clone(),
                prefix_field => prefixes,
                type_field => DocType::GrokMessage.as_str(),
                created_at_field => msg.created_at.timestamp(),
                metadata_field => metadata.to_string(),
            ))?;
            count += 1;
        }

        info!("Indexed {} Grok messages", count);
        Ok(count)
    }

    /// Search the index
    pub fn search(
        &self,
        query_str: &str,
        doc_types: Option<&[DocType]>,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let searcher = self.reader.searcher();
        let (id_field, text_field, _, type_field, created_at_field, metadata_field) = self.get_fields();

        // Build query
        let query_parser = QueryParser::for_index(&self.index, vec![text_field]);
        let base_query = query_parser
            .parse_query(query_str)
            .unwrap_or_else(|_| {
                // Fallback to term query if parsing fails
                Box::new(tantivy::query::AllQuery)
            });

        // Apply type filter if specified
        let query: Box<dyn Query> = if let Some(types) = doc_types {
            let type_queries: Vec<(Occur, Box<dyn Query>)> = types
                .iter()
                .map(|t| {
                    (
                        Occur::Should,
                        Box::new(TermQuery::new(
                            Term::from_field_text(type_field, t.as_str()),
                            IndexRecordOption::Basic,
                        )) as Box<dyn Query>,
                    )
                })
                .collect();

            let type_filter = BooleanQuery::new(type_queries);

            Box::new(BooleanQuery::new(vec![
                (Occur::Must, base_query),
                (Occur::Must, Box::new(type_filter)),
            ]))
        } else {
            base_query
        };

        // Execute search
        let top_docs = searcher.search(&query, &TopDocs::with_limit(limit))?;

        // Collect results
        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_address)?;

            let id = doc
                .get_first(id_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let text = doc
                .get_first(text_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let doc_type_str = doc
                .get_first(type_field)
                .and_then(|v| v.as_str())
                .unwrap_or("tweet");

            let created_at_ts = doc
                .get_first(created_at_field)
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let metadata_str = doc
                .get_first(metadata_field)
                .and_then(|v| v.as_str())
                .unwrap_or("{}");

            let result_type = match doc_type_str {
                "tweet" => SearchResultType::Tweet,
                "like" => SearchResultType::Like,
                "dm" => SearchResultType::DirectMessage,
                "grok" => SearchResultType::GrokMessage,
                _ => SearchResultType::Tweet,
            };

            results.push(SearchResult {
                result_type,
                id,
                text,
                created_at: DateTime::from_timestamp(created_at_ts, 0)
                    .unwrap_or_else(|| Utc::now()),
                score,
                highlights: vec![], // TODO: implement highlighting
                metadata: serde_json::from_str(metadata_str).unwrap_or_default(),
            });
        }

        Ok(results)
    }

    /// Get document count
    pub fn doc_count(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    /// Delete all documents and reset the index
    pub fn clear(&self) -> Result<()> {
        let mut writer = self.writer(50_000_000)?;
        writer.delete_all_documents()?;
        writer.commit()?;
        self.reload()?;
        Ok(())
    }
}

/// Generate prefix terms for edge n-gram style matching
fn generate_prefixes(text: &str) -> String {
    let words: Vec<&str> = text
        .split_whitespace()
        .filter(|w| w.len() >= 2)
        .take(50) // Limit to prevent huge documents
        .collect();

    let mut prefixes = Vec::new();
    for word in words {
        let word_lower = word.to_lowercase();
        // Generate 2-char to full-length prefixes
        for len in 2..=word_lower.len().min(15) {
            if let Some(prefix) = word_lower.get(..len) {
                prefixes.push(prefix.to_string());
            }
        }
    }

    prefixes.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_prefixes() {
        let text = "hello world";
        let prefixes = generate_prefixes(text);
        assert!(prefixes.contains("he"));
        assert!(prefixes.contains("hel"));
        assert!(prefixes.contains("hell"));
        assert!(prefixes.contains("hello"));
        assert!(prefixes.contains("wo"));
        assert!(prefixes.contains("wor"));
        assert!(prefixes.contains("worl"));
        assert!(prefixes.contains("world"));
    }

    #[test]
    fn test_search_engine() {
        let engine = SearchEngine::open_memory().unwrap();
        let mut writer = engine.writer(15_000_000).unwrap();

        // Index a tweet
        let tweets = vec![Tweet {
            id: "123".to_string(),
            created_at: Utc::now(),
            full_text: "Hello world this is a test tweet".to_string(),
            source: Some("test".to_string()),
            favorite_count: 0,
            retweet_count: 0,
            lang: Some("en".to_string()),
            in_reply_to_status_id: None,
            in_reply_to_user_id: None,
            in_reply_to_screen_name: None,
            is_retweet: false,
            hashtags: vec![],
            user_mentions: vec![],
            urls: vec![],
            media: vec![],
        }];

        engine.index_tweets(&mut writer, &tweets).unwrap();
        writer.commit().unwrap();
        engine.reload().unwrap();

        // Search
        let results = engine.search("hello", None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "123");
    }
}
