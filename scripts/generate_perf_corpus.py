#!/usr/bin/env python3
"""
Deterministic test corpus generator for xf performance benchmarks.

Generates a reproducible test corpus for xf benchmarks in the X archive format.
The corpus is deterministic: given the same seed, it produces identical output.

Usage:
    python3 scripts/generate_perf_corpus.py [--seed SEED] [--output-dir DIR] [--scale SCALE]

Output (default scale=1.0):
    - tweets.js: 10,000 tweets
    - like.js: 5,000 likes
    - direct-messages.js: 2,000 messages in 100 conversations
    - grok-chat-item.js: 500 Grok messages

Scales:
    0.1 = 1/10th size (for quick tests)
    1.0 = standard corpus (17,500 records)
    5.0 = 5x size (for stress tests)
"""

import argparse
import hashlib
import json
import random
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path


# Deterministic seed for reproducibility
DEFAULT_SEED = 42
GENERATION_TIMESTAMP = datetime(2026, 1, 1, 12, 0, 0, tzinfo=timezone.utc)

# Sample text pools for realistic content diversity
SAMPLE_TEXTS = [
    "Just finished reading an amazing book about Rust programming!",
    "The weather today is absolutely beautiful, perfect for a walk.",
    "Working on a new machine learning project using Python and TensorFlow.",
    "Can't believe how fast this new search engine is! Sub-millisecond queries!",
    "Exploring the latest features in async Rust - the ecosystem is maturing nicely.",
    "Had a great coffee at the local cafe this morning. Highly recommend!",
    "The concert last night was incredible. Best live performance I've seen.",
    "Just deployed a new microservice architecture using Kubernetes.",
    "Learning about distributed systems and consensus algorithms today.",
    "The sunset from my balcony is breathtaking right now.",
    "Finally got around to setting up my home lab with Proxmox.",
    "Been playing around with WebAssembly - the future of web development?",
    "This new VS Code extension is a game changer for productivity.",
    "Reading papers on transformer architectures. So much to learn!",
    "Just discovered a fantastic open source project on GitHub.",
]

UNICODE_TEXTS = [
    "Testing unicode support: cafe\u0301 \u2615\ufe0f and emojis \ud83c\udf89\ud83d\ude80",
    "\u65e5\u672c\u8a9e\u3067\u30c4\u30a4\u30fc\u30c8\u3092\u66f8\u304f - Japanese tweet test",
    "\u0645\u0631\u062d\u0628\u0627 \u0628\u0627\u0644\u0639\u0627\u0644\u0645 - Arabic RTL text support",
    "\u4e2d\u6587\u6d4b\u8bd5 - Chinese character support test",
    "\ud83d\udc68\u200d\ud83d\udcbb\ud83d\udc69\u200d\ud83d\udcbb Code together! \ud83d\ude80\u2728\ud83c\udf1f",
]

HASHTAGS = ["rust", "programming", "tech", "coding", "software", "dev", "opensource",
            "machinelearning", "ai", "webdev", "linux", "python", "javascript", "cloud"]

MENTIONS = ["@rustlang", "@github", "@microsoft", "@google", "@anthropic",
            "@elonmusk", "@openai", "@vercel", "@nodejs", "@typescriptlang"]

DM_TOPICS = [
    ("meeting tomorrow", "What time works for you?"),
    ("project update", "Here's the latest status on our work."),
    ("quick question", "Do you have a moment to chat?"),
    ("lunch plans", "Want to grab lunch tomorrow?"),
    ("code review", "Can you take a look at my PR?"),
]

GROK_TOPICS = [
    ("How does async/await work in Rust?", "Let me explain async/await in Rust..."),
    ("What's the best way to learn ML?", "Here are some great resources for machine learning..."),
    ("Explain quantum computing", "Quantum computing uses quantum mechanical phenomena..."),
    ("How do transformers work?", "Transformer architectures use self-attention..."),
    ("What is Rust's ownership model?", "Rust's ownership model ensures memory safety..."),
]


def deterministic_hash(s: str) -> int:
    """Generate a deterministic hash from a string."""
    return int(hashlib.sha256(s.encode()).hexdigest()[:16], 16)


def generate_timestamp(rng: random.Random, base: datetime, i: int, total: int) -> str:
    """Generate a timestamp in X archive format."""
    # Spread timestamps over 5 years
    days_offset = int((total - i) * 365 * 5 / total)
    hours_offset = rng.randint(0, 23)
    minutes_offset = rng.randint(0, 59)
    dt = base - timedelta(days=days_offset, hours=hours_offset, minutes=minutes_offset)
    # X format: "Fri Jan 09 15:12:21 +0000 2026"
    return dt.strftime("%a %b %d %H:%M:%S +0000 %Y")


def generate_iso_timestamp(rng: random.Random, base: datetime, i: int, total: int) -> str:
    """Generate ISO 8601 timestamp."""
    days_offset = int((total - i) * 365 * 5 / total)
    hours_offset = rng.randint(0, 23)
    dt = base - timedelta(days=days_offset, hours=hours_offset)
    return dt.strftime("%Y-%m-%dT%H:%M:%S.000Z")


def generate_tweets(rng: random.Random, count: int) -> list[dict]:
    """Generate deterministic tweet data."""
    base_time = GENERATION_TIMESTAMP
    tweets = []

    for i in range(count):
        # Deterministic text selection with some variation
        text_idx = i % len(SAMPLE_TEXTS)
        text = SAMPLE_TEXTS[text_idx]

        # Add unicode content every 50th tweet
        if i % 50 == 0 and i > 0:
            text = UNICODE_TEXTS[i % len(UNICODE_TEXTS)]

        # Add hashtags deterministically
        num_hashtags = (i % 4)
        hashtags_used = [HASHTAGS[(i + j) % len(HASHTAGS)] for j in range(num_hashtags)]
        if hashtags_used:
            text += " " + " ".join(f"#{h}" for h in hashtags_used)

        # Add mentions deterministically
        if i % 10 == 0:
            mention = MENTIONS[i % len(MENTIONS)]
            text = f"{mention} {text}"

        # Vary text length (1-280 chars)
        if i % 100 == 0:
            text = text[:rng.randint(1, 50)]  # Short tweets
        elif len(text) > 280:
            text = text[:280]

        # Deterministic engagement metrics with realistic distribution
        favorites = int((count - i) * rng.random() * 0.1)  # More recent = less likes (newer)
        retweets = int(favorites * 0.3 * rng.random())

        tweet = {
            "tweet": {
                "id": str(1_000_000_000_000 + i),
                "id_str": str(1_000_000_000_000 + i),
                "created_at": generate_timestamp(rng, base_time, i, count),
                "full_text": text,
                "truncated": False,
                "source": '<a href="https://mobile.x.com" rel="nofollow">X for iPhone</a>',
                "favorite_count": str(favorites),
                "retweet_count": str(retweets),
                "lang": "en",
                "entities": {
                    "hashtags": [{"text": h, "indices": [0, len(h)+1]} for h in hashtags_used],
                    "user_mentions": [],
                    "urls": [],
                },
            }
        }

        # Add reply chain for some tweets
        if i % 20 == 0 and i > 0:
            reply_to_id = str(1_000_000_000_000 + i - rng.randint(1, min(i, 10)))
            tweet["tweet"]["in_reply_to_status_id"] = reply_to_id
            tweet["tweet"]["in_reply_to_status_id_str"] = reply_to_id
            tweet["tweet"]["in_reply_to_user_id"] = str(100000 + (i % 100))
            tweet["tweet"]["in_reply_to_screen_name"] = f"user_{i % 100}"

        tweets.append(tweet)

    return tweets


def generate_likes(rng: random.Random, count: int) -> list[dict]:
    """Generate deterministic like data."""
    likes = []

    for i in range(count):
        text_idx = i % len(SAMPLE_TEXTS)
        text = SAMPLE_TEXTS[text_idx]

        # Some likes have missing text (like real X exports)
        if i % 10 == 0:
            text = None

        like = {
            "like": {
                "tweetId": str(2_000_000_000_000 + i),
                "fullText": text,
                "expandedUrl": f"https://x.com/user_{i % 1000}/status/{2_000_000_000_000 + i}",
            }
        }
        likes.append(like)

    return likes


def generate_direct_messages(rng: random.Random, message_count: int, convo_count: int) -> list[dict]:
    """Generate deterministic DM data with conversations."""
    base_time = GENERATION_TIMESTAMP
    conversations = []
    messages_per_convo = message_count // convo_count

    for convo_idx in range(convo_count):
        convo_id = str(3_000_000_000_000 + convo_idx)
        participant_ids = [str(100000 + convo_idx), str(200000 + convo_idx)]
        messages = []

        topic_idx = convo_idx % len(DM_TOPICS)
        topic_q, topic_a = DM_TOPICS[topic_idx]

        for msg_idx in range(messages_per_convo):
            global_idx = convo_idx * messages_per_convo + msg_idx
            sender_idx = msg_idx % 2

            # Alternate between participants
            if msg_idx == 0:
                text = f"Hey! {topic_q}"
            elif msg_idx == 1:
                text = topic_a
            else:
                texts = [
                    "That makes sense, thanks!",
                    "Got it, I'll check that out.",
                    "Interesting perspective.",
                    "Can you elaborate on that?",
                    "Perfect, that helps a lot!",
                ]
                text = texts[msg_idx % len(texts)]

            message = {
                "messageCreate": {
                    "id": str(4_000_000_000_000 + global_idx),
                    "senderId": participant_ids[sender_idx],
                    "recipientId": participant_ids[1 - sender_idx],
                    "text": text,
                    "createdAt": generate_iso_timestamp(rng, base_time, global_idx, message_count),
                    "mediaUrls": [],
                    "urls": [],
                }
            }
            messages.append(message)

        conversation = {
            "dmConversation": {
                "conversationId": convo_id,
                "messages": messages,
            }
        }
        conversations.append(conversation)

    return conversations


def generate_grok_messages(rng: random.Random, count: int) -> list[dict]:
    """Generate deterministic Grok chat items."""
    base_time = GENERATION_TIMESTAMP
    messages = []

    for i in range(count):
        topic_idx = (i // 2) % len(GROK_TOPICS)
        is_user = i % 2 == 0

        if is_user:
            text = GROK_TOPICS[topic_idx][0]
            sender = "user"
        else:
            text = GROK_TOPICS[topic_idx][1]
            sender = "grok"

        chat_id = str(5_000_000_000_000 + (i // 10))

        message = {
            "grokChatItem": {
                "chatId": chat_id,
                "message": text,
                "sender": sender,
                "createdAt": generate_iso_timestamp(rng, base_time, i, count),
                "grokMode": "default",
            }
        }
        messages.append(message)

    return messages


def write_js_file(path: Path, var_name: str, data: list | dict) -> str:
    """Write data in X archive JavaScript format and return SHA256."""
    content = f"window.YTD.{var_name}.part0 = {json.dumps(data, indent=2)}"
    path.write_text(content, encoding="utf-8")
    return hashlib.sha256(content.encode()).hexdigest()


def generate_manifest(
    tweet_count: int,
    like_count: int,
    dm_message_count: int,
    grok_count: int,
    total_size: int,
) -> dict:
    """Generate the manifest.js content that xf requires."""
    return {
        "userInfo": {
            "accountId": "123456789",
            "userName": "perf_test_user",
            "displayName": "Performance Test User",
        },
        "archiveInfo": {
            "sizeBytes": str(total_size),
            "generationDate": GENERATION_TIMESTAMP.strftime("%Y-%m-%dT%H:%M:%S.000Z"),
            "isPartialArchive": False,
            "maxPartSizeBytes": "53687091200",
        },
        "dataTypes": {
            "tweets": {
                "files": [
                    {
                        "fileName": "data/tweets.js",
                        "globalName": "YTD.tweets.part0",
                        "count": str(tweet_count),
                    }
                ]
            },
            "like": {
                "files": [
                    {
                        "fileName": "data/like.js",
                        "globalName": "YTD.like.part0",
                        "count": str(like_count),
                    }
                ]
            },
            "directMessages": {
                "files": [
                    {
                        "fileName": "data/direct-messages.js",
                        "globalName": "YTD.direct_messages.part0",
                        "count": str(dm_message_count),
                    }
                ]
            },
            "grokChatItem": {
                "files": [
                    {
                        "fileName": "data/grok-chat-item.js",
                        "globalName": "YTD.grok_chat_item.part0",
                        "count": str(grok_count),
                    }
                ]
            },
        },
    }


def main():
    parser = argparse.ArgumentParser(description="Generate deterministic test corpus for xf")
    parser.add_argument("--seed", type=int, default=DEFAULT_SEED,
                       help=f"Random seed for reproducibility (default: {DEFAULT_SEED})")
    parser.add_argument("--output-dir", type=Path,
                       default=Path("tests/fixtures/perf_corpus"),
                       help="Output directory for corpus files")
    parser.add_argument("--scale", type=float, default=1.0,
                       help="Scale factor (1.0 = 17,500 records)")
    args = parser.parse_args()

    # Initialize deterministic RNG
    rng = random.Random(args.seed)

    # Calculate counts based on scale
    tweet_count = int(10_000 * args.scale)
    like_count = int(5_000 * args.scale)
    dm_message_count = int(2_000 * args.scale)
    dm_convo_count = int(100 * args.scale)
    grok_count = int(500 * args.scale)

    # Ensure output directory exists
    output_dir = args.output_dir
    data_dir = output_dir / "data"
    data_dir.mkdir(parents=True, exist_ok=True)

    print(f"Generating corpus with seed={args.seed}, scale={args.scale}")
    print(f"  Tweets: {tweet_count}")
    print(f"  Likes: {like_count}")
    print(f"  DM Messages: {dm_message_count} in {dm_convo_count} conversations")
    print(f"  Grok Messages: {grok_count}")

    # Generate data
    print("\nGenerating tweets...", end=" ", flush=True)
    tweets = generate_tweets(rng, tweet_count)
    tweet_hash = write_js_file(data_dir / "tweets.js", "tweets", tweets)
    print(f"done ({len(tweets)} records)")

    print("Generating likes...", end=" ", flush=True)
    likes = generate_likes(rng, like_count)
    like_hash = write_js_file(data_dir / "like.js", "like", likes)
    print(f"done ({len(likes)} records)")

    print("Generating DMs...", end=" ", flush=True)
    dms = generate_direct_messages(rng, dm_message_count, dm_convo_count)
    dm_hash = write_js_file(data_dir / "direct-messages.js", "direct_messages", dms)
    print(f"done ({dm_message_count} messages in {len(dms)} conversations)")

    print("Generating Grok messages...", end=" ", flush=True)
    grok = generate_grok_messages(rng, grok_count)
    grok_hash = write_js_file(data_dir / "grok-chat-item.js", "grok_chat_item", grok)
    print(f"done ({len(grok)} messages)")

    # Calculate total size and generate X archive manifest.js
    total_records = len(tweets) + len(likes) + dm_message_count + len(grok)
    total_size = sum(
        (data_dir / fname).stat().st_size
        for fname in [
            "tweets.js",
            "like.js",
            "direct-messages.js",
            "grok-chat-item.js",
        ]
    )
    archive_manifest = generate_manifest(
        len(tweets),
        len(likes),
        dm_message_count,
        len(grok),
        total_size,
    )
    print("Generating manifest.js...", end=" ", flush=True)
    manifest_js_hash = write_js_file(data_dir / "manifest.js", "manifest", archive_manifest)
    print("done")

    # Write corpus manifest with checksums
    manifest = {
        "seed": args.seed,
        "scale": args.scale,
        "generated_at": GENERATION_TIMESTAMP.isoformat(),
        "files": {
            "manifest.js": {"records": 1, "sha256": manifest_js_hash},
            "tweets.js": {"records": len(tweets), "sha256": tweet_hash},
            "like.js": {"records": len(likes), "sha256": like_hash},
            "direct-messages.js": {"records": dm_message_count, "sha256": dm_hash},
            "grok-chat-item.js": {"records": len(grok), "sha256": grok_hash},
        },
        "total_records": total_records,
    }

    manifest_path = output_dir / "corpus_manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2))
    print(f"\nManifest written to {manifest_path}")

    # Print checksums for verification
    print("\nFile checksums (SHA256):")
    for fname, info in manifest["files"].items():
        print(f"  {fname}: {info['sha256'][:16]}...")

    print(f"\nTotal records: {manifest['total_records']}")
    print(f"Corpus generated successfully in {output_dir}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
