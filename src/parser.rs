//! X archive data parser.
//!
//! Handles parsing the JavaScript-wrapped JSON format used in X data exports.
//! Files are formatted as: `window.YTD.<datatype>.part0 = [...]`

use crate::model::*;
use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use rayon::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

/// Parser for X archive data
pub struct ArchiveParser {
    archive_path: std::path::PathBuf,
}

impl ArchiveParser {
    pub fn new(archive_path: impl AsRef<Path>) -> Self {
        Self {
            archive_path: archive_path.as_ref().to_path_buf(),
        }
    }

    /// Parse the JavaScript file format and extract JSON
    fn parse_js_file(&self, content: &str) -> Result<Value> {
        // Format: window.YTD.<type>.part<n> = [...]
        // We need to extract everything after the " = "
        let equals_pos = content
            .find(" = ")
            .context("Invalid JS file format: no ' = ' found")?;

        let json_str = &content[equals_pos + 3..];
        serde_json::from_str(json_str).context("Failed to parse JSON from JS file")
    }

    /// Read and parse a JS data file
    fn read_data_file(&self, filename: &str) -> Result<Value> {
        let path = self.archive_path.join("data").join(filename);
        if !path.exists() {
            return Ok(Value::Array(vec![]));
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        self.parse_js_file(&content)
    }

    /// Parse X's date format: "Fri Jan 09 15:12:21 +0000 2026"
    fn parse_x_date(date_str: &str) -> Option<DateTime<Utc>> {
        // X format: "Fri Jan 09 15:12:21 +0000 2026"
        DateTime::parse_from_str(date_str, "%a %b %d %H:%M:%S %z %Y")
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    }

    /// Parse ISO 8601 date format
    fn parse_iso_date(date_str: &str) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(date_str)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    }

    /// Parse archive metadata from manifest.js
    pub fn parse_manifest(&self) -> Result<ArchiveInfo> {
        let data = self.read_data_file("manifest.js")?;

        let user_info = &data["userInfo"];
        let archive_info = &data["archiveInfo"];

        Ok(ArchiveInfo {
            account_id: user_info["accountId"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            username: user_info["userName"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            display_name: user_info["displayName"].as_str().map(String::from),
            archive_size_bytes: archive_info["sizeBytes"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            generation_date: archive_info["generationDate"]
                .as_str()
                .and_then(Self::parse_iso_date)
                .unwrap_or_else(Utc::now),
            is_partial: archive_info["isPartialArchive"]
                .as_bool()
                .unwrap_or(false),
        })
    }

    /// Parse all tweets from tweets.js
    pub fn parse_tweets(&self) -> Result<Vec<Tweet>> {
        info!("Parsing tweets.js...");
        let data = self.read_data_file("tweets.js")?;

        let tweets: Vec<Tweet> = data
            .as_array()
            .unwrap_or(&vec![])
            .par_iter()
            .filter_map(|item| {
                let tweet = &item["tweet"];
                Some(Tweet {
                    id: tweet["id_str"].as_str()?.to_string(),
                    created_at: tweet["created_at"]
                        .as_str()
                        .and_then(Self::parse_x_date)?,
                    full_text: tweet["full_text"].as_str()?.to_string(),
                    source: tweet["source"].as_str().map(|s| {
                        // Extract text from HTML anchor tag
                        s.split('>')
                            .nth(1)
                            .and_then(|s| s.split('<').next())
                            .unwrap_or(s)
                            .to_string()
                    }),
                    favorite_count: tweet["favorite_count"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0),
                    retweet_count: tweet["retweet_count"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0),
                    lang: tweet["lang"].as_str().map(String::from),
                    in_reply_to_status_id: tweet["in_reply_to_status_id_str"]
                        .as_str()
                        .map(String::from),
                    in_reply_to_user_id: tweet["in_reply_to_user_id_str"]
                        .as_str()
                        .map(String::from),
                    in_reply_to_screen_name: tweet["in_reply_to_screen_name"]
                        .as_str()
                        .map(String::from),
                    is_retweet: tweet["retweeted"].as_bool().unwrap_or(false),
                    hashtags: Self::parse_hashtags(&tweet["entities"]["hashtags"]),
                    user_mentions: Self::parse_user_mentions(&tweet["entities"]["user_mentions"]),
                    urls: Self::parse_urls(&tweet["entities"]["urls"]),
                    media: Self::parse_media(&tweet["entities"]["media"]),
                })
            })
            .collect();

        info!("Parsed {} tweets", tweets.len());
        Ok(tweets)
    }

    fn parse_hashtags(value: &Value) -> Vec<String> {
        value
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|h| h["text"].as_str().map(String::from))
            .collect()
    }

    fn parse_user_mentions(value: &Value) -> Vec<UserMention> {
        value
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|m| {
                Some(UserMention {
                    id: m["id_str"].as_str()?.to_string(),
                    screen_name: m["screen_name"].as_str()?.to_string(),
                    name: m["name"].as_str().map(String::from),
                })
            })
            .collect()
    }

    fn parse_urls(value: &Value) -> Vec<TweetUrl> {
        value
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|u| {
                Some(TweetUrl {
                    url: u["url"].as_str()?.to_string(),
                    expanded_url: u["expanded_url"].as_str().map(String::from),
                    display_url: u["display_url"].as_str().map(String::from),
                })
            })
            .collect()
    }

    fn parse_media(value: &Value) -> Vec<TweetMedia> {
        value
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|m| {
                Some(TweetMedia {
                    id: m["id_str"].as_str()?.to_string(),
                    media_type: m["type"].as_str().unwrap_or("photo").to_string(),
                    url: m["media_url_https"]
                        .as_str()
                        .or_else(|| m["media_url"].as_str())?
                        .to_string(),
                    local_path: None,
                })
            })
            .collect()
    }

    /// Parse all likes from like.js
    pub fn parse_likes(&self) -> Result<Vec<Like>> {
        info!("Parsing like.js...");
        let data = self.read_data_file("like.js")?;

        let likes: Vec<Like> = data
            .as_array()
            .unwrap_or(&vec![])
            .par_iter()
            .filter_map(|item| {
                let like = &item["like"];
                Some(Like {
                    tweet_id: like["tweetId"].as_str()?.to_string(),
                    full_text: like["fullText"].as_str().map(String::from),
                    expanded_url: like["expandedUrl"].as_str().map(String::from),
                })
            })
            .collect();

        info!("Parsed {} likes", likes.len());
        Ok(likes)
    }

    /// Parse direct messages from direct-messages.js
    pub fn parse_direct_messages(&self) -> Result<Vec<DmConversation>> {
        info!("Parsing direct-messages.js...");
        let data = self.read_data_file("direct-messages.js")?;

        let conversations: Vec<DmConversation> = data
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|item| {
                let conv = &item["dmConversation"];
                let messages: Vec<DirectMessage> = conv["messages"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|msg| {
                        let mc = &msg["messageCreate"];
                        Some(DirectMessage {
                            id: mc["id"].as_str()?.to_string(),
                            sender_id: mc["senderId"].as_str()?.to_string(),
                            recipient_id: mc["recipientId"].as_str()?.to_string(),
                            text: mc["text"].as_str()?.to_string(),
                            created_at: mc["createdAt"]
                                .as_str()
                                .and_then(Self::parse_iso_date)?,
                            urls: Self::parse_dm_urls(&mc["urls"]),
                            media_urls: mc["mediaUrls"]
                                .as_array()
                                .unwrap_or(&vec![])
                                .iter()
                                .filter_map(|u| u.as_str().map(String::from))
                                .collect(),
                        })
                    })
                    .collect();

                Some(DmConversation {
                    conversation_id: conv["conversationId"].as_str()?.to_string(),
                    messages,
                })
            })
            .collect();

        let total_messages: usize = conversations.iter().map(|c| c.messages.len()).sum();
        info!(
            "Parsed {} DM conversations with {} total messages",
            conversations.len(),
            total_messages
        );
        Ok(conversations)
    }

    fn parse_dm_urls(value: &Value) -> Vec<TweetUrl> {
        value
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|u| {
                Some(TweetUrl {
                    url: u["url"].as_str()?.to_string(),
                    expanded_url: u["expanded"].as_str().map(String::from),
                    display_url: u["display"].as_str().map(String::from),
                })
            })
            .collect()
    }

    /// Parse followers from follower.js
    pub fn parse_followers(&self) -> Result<Vec<Follower>> {
        info!("Parsing follower.js...");
        let data = self.read_data_file("follower.js")?;

        let followers: Vec<Follower> = data
            .as_array()
            .unwrap_or(&vec![])
            .par_iter()
            .filter_map(|item| {
                let f = &item["follower"];
                Some(Follower {
                    account_id: f["accountId"].as_str()?.to_string(),
                    user_link: f["userLink"].as_str().map(String::from),
                })
            })
            .collect();

        info!("Parsed {} followers", followers.len());
        Ok(followers)
    }

    /// Parse following from following.js
    pub fn parse_following(&self) -> Result<Vec<Following>> {
        info!("Parsing following.js...");
        let data = self.read_data_file("following.js")?;

        let following: Vec<Following> = data
            .as_array()
            .unwrap_or(&vec![])
            .par_iter()
            .filter_map(|item| {
                let f = &item["following"];
                Some(Following {
                    account_id: f["accountId"].as_str()?.to_string(),
                    user_link: f["userLink"].as_str().map(String::from),
                })
            })
            .collect();

        info!("Parsed {} following", following.len());
        Ok(following)
    }

    /// Parse blocks from block.js
    pub fn parse_blocks(&self) -> Result<Vec<Block>> {
        info!("Parsing block.js...");
        let data = self.read_data_file("block.js")?;

        let blocks: Vec<Block> = data
            .as_array()
            .unwrap_or(&vec![])
            .par_iter()
            .filter_map(|item| {
                let b = &item["blocking"];
                Some(Block {
                    account_id: b["accountId"].as_str()?.to_string(),
                    user_link: b["userLink"].as_str().map(String::from),
                })
            })
            .collect();

        info!("Parsed {} blocks", blocks.len());
        Ok(blocks)
    }

    /// Parse mutes from mute.js
    pub fn parse_mutes(&self) -> Result<Vec<Mute>> {
        info!("Parsing mute.js...");
        let data = self.read_data_file("mute.js")?;

        let mutes: Vec<Mute> = data
            .as_array()
            .unwrap_or(&vec![])
            .par_iter()
            .filter_map(|item| {
                let m = &item["muting"];
                Some(Mute {
                    account_id: m["accountId"].as_str()?.to_string(),
                    user_link: m["userLink"].as_str().map(String::from),
                })
            })
            .collect();

        info!("Parsed {} mutes", mutes.len());
        Ok(mutes)
    }

    /// Parse account info from account.js
    pub fn parse_account(&self) -> Result<Option<Account>> {
        info!("Parsing account.js...");
        let data = self.read_data_file("account.js")?;

        let account = data.as_array().and_then(|arr| arr.first()).map(|item| {
            let a = &item["account"];
            Account {
                account_id: a["accountId"].as_str().unwrap_or_default().to_string(),
                username: a["username"].as_str().unwrap_or_default().to_string(),
                display_name: a["accountDisplayName"].as_str().map(String::from),
                email: a["email"].as_str().map(String::from),
                created_at: a["createdAt"].as_str().and_then(Self::parse_iso_date),
                created_via: a["createdVia"].as_str().map(String::from),
            }
        });

        Ok(account)
    }

    /// Parse profile info from profile.js
    pub fn parse_profile(&self) -> Result<Option<Profile>> {
        info!("Parsing profile.js...");
        let data = self.read_data_file("profile.js")?;

        let profile = data.as_array().and_then(|arr| arr.first()).map(|item| {
            let p = &item["profile"];
            let desc = &p["description"];
            Profile {
                bio: desc["bio"].as_str().map(String::from),
                website: desc["website"].as_str().map(String::from),
                location: desc["location"].as_str().map(String::from),
                avatar_url: p["avatarMediaUrl"].as_str().map(String::from),
                header_url: p["headerMediaUrl"].as_str().map(String::from),
            }
        });

        Ok(profile)
    }

    /// Parse Grok chat messages from grok-chat-item.js
    pub fn parse_grok_messages(&self) -> Result<Vec<GrokMessage>> {
        info!("Parsing grok-chat-item.js...");
        let data = self.read_data_file("grok-chat-item.js")?;

        let messages: Vec<GrokMessage> = data
            .as_array()
            .unwrap_or(&vec![])
            .par_iter()
            .filter_map(|item| {
                let g = &item["grokChatItem"];
                Some(GrokMessage {
                    chat_id: g["chatId"].as_str()?.to_string(),
                    message: g["message"].as_str()?.to_string(),
                    sender: g["sender"].as_str().unwrap_or("unknown").to_string(),
                    created_at: g["createdAt"].as_str().and_then(Self::parse_iso_date)?,
                    grok_mode: g["grokMode"].as_str().map(String::from),
                })
            })
            .collect();

        info!("Parsed {} Grok messages", messages.len());
        Ok(messages)
    }

    /// List all available data files in the archive
    pub fn list_data_files(&self) -> Result<Vec<String>> {
        let data_path = self.archive_path.join("data");
        let mut files = Vec::new();

        for entry in WalkDir::new(&data_path).max_depth(1) {
            let entry = entry?;
            if entry.file_type().is_file() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".js") {
                        files.push(name.to_string());
                    }
                }
            }
        }

        files.sort();
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_x_date() {
        let date = ArchiveParser::parse_x_date("Fri Jan 09 15:12:21 +0000 2026");
        assert!(date.is_some());
        let dt = date.unwrap();
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 9);
    }

    #[test]
    fn test_parse_iso_date() {
        let date = ArchiveParser::parse_iso_date("2025-11-06T23:32:43.358Z");
        assert!(date.is_some());
    }
}
