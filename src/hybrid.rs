//! Hybrid search with Reciprocal Rank Fusion (RRF).
//!
//! Combines lexical (keyword) and semantic (vector) search results
//! using RRF scoring for optimal relevance.
//!
//! # Algorithm
//!
//! RRF score = Σ 1/(K + rank + 1)
//!
//! Where:
//! - K = 60 (constant, empirically optimal)
//! - rank = position in result list (0-indexed)
//!
//! Results appearing in both lists get scores from both, naturally
//! boosting documents that match both keyword and meaning.
//!
//! # Tie-Breaking
//!
//! For deterministic ordering:
//! 1. RRF score (descending)
//! 2. Appears in both lists (bonus)
//! 3. Document ID (ascending)

use crate::model::{SearchResult, SearchResultType};
use crate::vector::VectorSearchResult;
use clap::ValueEnum;
use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DocKey<'a> {
    id: &'a str,
    doc_type: &'a str,
}

impl<'a> DocKey<'a> {
    #[allow(clippy::missing_const_for_fn)]
    fn new(id: &'a str, doc_type: &'a str) -> Self {
        Self { id, doc_type }
    }
}

/// RRF constant K. Empirically, K=60 works well for most use cases.
const RRF_K: f32 = 60.0;

/// Multiplier for candidate fetching. Fetch 3x more candidates from each
/// source to ensure good coverage after fusion.
pub const CANDIDATE_MULTIPLIER: usize = 3;

/// Search mode for hybrid search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum SearchMode {
    /// Keyword-only search using BM25.
    Lexical,
    /// Semantic-only search using vector similarity.
    Semantic,
    /// Hybrid search combining lexical and semantic with RRF (default).
    #[default]
    Hybrid,
}

impl std::fmt::Display for SearchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lexical => write!(f, "lexical"),
            Self::Semantic => write!(f, "semantic"),
            Self::Hybrid => write!(f, "hybrid"),
        }
    }
}

impl std::str::FromStr for SearchMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "lexical" | "keyword" | "bm25" => Ok(Self::Lexical),
            "semantic" | "vector" | "embedding" => Ok(Self::Semantic),
            "hybrid" | "rrf" | "both" => Ok(Self::Hybrid),
            _ => Err(format!(
                "unknown search mode: '{s}'. Use 'lexical', 'semantic', or 'hybrid'"
            )),
        }
    }
}

/// Hybrid score tracking for RRF fusion.
#[derive(Debug, Default, Clone)]
struct HybridScore {
    /// RRF score (sum of 1/(K+rank+1) from each list).
    rrf: f32,
    /// Rank in lexical results (if present).
    lexical_rank: Option<usize>,
    /// Rank in semantic results (if present).
    semantic_rank: Option<usize>,
}

/// Fused search hit combining information from both sources.
#[derive(Debug, Clone)]
pub struct FusedHit<'a> {
    /// Document ID.
    pub doc_id: &'a str,
    /// Document type.
    pub doc_type: &'a str,
    /// Fused RRF score.
    pub score: f32,
    /// Index into the lexical results (if present).
    pub lexical_rank: Option<usize>,
    /// Whether this hit appeared in both lexical and semantic results.
    pub in_both: bool,
}

const fn result_type_str(result_type: SearchResultType) -> &'static str {
    match result_type {
        SearchResultType::Tweet => "tweet",
        SearchResultType::Like => "like",
        SearchResultType::DirectMessage => "dm",
        SearchResultType::GrokMessage => "grok",
    }
}

/// Fuse lexical and semantic search results using RRF.
///
/// # Arguments
///
/// * `lexical` - Results from keyword (Tantivy) search
/// * `semantic` - Results from vector similarity search
/// * `limit` - Maximum number of results to return
/// * `offset` - Number of results to skip (for pagination)
///
/// # Returns
///
/// Fused results sorted by RRF score, with deterministic tie-breaking.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn rrf_fuse<'a>(
    lexical: &'a [SearchResult],
    semantic: &'a [VectorSearchResult],
    limit: usize,
    offset: usize,
) -> Vec<FusedHit<'a>> {
    if limit == 0 {
        return Vec::new();
    }

    let mut scores: HashMap<DocKey<'a>, HybridScore> = HashMap::new();

    // Process lexical results (rank 0, 1, 2, ...)
    for (rank, hit) in lexical.iter().enumerate() {
        let doc_type = result_type_str(hit.result_type);
        let key = DocKey::new(hit.id.as_str(), doc_type);
        let entry = scores.entry(key).or_default();
        entry.rrf += 1.0 / (RRF_K + rank as f32 + 1.0);
        entry.lexical_rank = Some(rank);
    }

    // Process semantic results (rank 0, 1, 2, ...)
    for (rank, hit) in semantic.iter().enumerate() {
        let key = DocKey::new(hit.doc_id.as_str(), hit.doc_type.as_str());
        let entry = scores.entry(key).or_default();
        entry.rrf += 1.0 / (RRF_K + rank as f32 + 1.0);
        entry.semantic_rank = Some(rank);
    }

    // Convert to fused hits
    let mut fused: Vec<FusedHit<'a>> = scores
        .into_iter()
        .map(|(key, score)| {
            let in_both = score.lexical_rank.is_some() && score.semantic_rank.is_some();
            FusedHit {
                doc_id: key.id,
                doc_type: key.doc_type,
                score: score.rrf,
                lexical_rank: score.lexical_rank,
                in_both,
            }
        })
        .collect();

    // Sort with deterministic tie-breaking
    fused.sort_by(|a, b| {
        // Level 1: RRF score (descending)
        b.score
            .total_cmp(&a.score)
            // Level 2: Prefer hits appearing in both lists
            .then_with(|| match (b.in_both, a.in_both) {
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                _ => Ordering::Equal,
            })
            // Level 3: Document ID (ascending) for determinism
            .then_with(|| a.doc_id.cmp(b.doc_id))
            .then_with(|| a.doc_type.cmp(b.doc_type))
    });

    // Apply offset and limit
    let start = offset.min(fused.len());
    let end = start.saturating_add(limit).min(fused.len());

    fused.into_iter().skip(start).take(end - start).collect()
}

/// Calculate the number of candidates to fetch for hybrid search.
///
/// Fetches 3x the limit from each source to ensure good coverage
/// after RRF fusion filters results.
#[must_use]
pub const fn candidate_count(limit: usize, offset: usize) -> usize {
    limit
        .saturating_add(offset)
        .saturating_mul(CANDIDATE_MULTIPLIER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SearchResultType;
    use chrono::Utc;

    fn make_lexical_hit(id: &str, score: f32, result_type: SearchResultType) -> SearchResult {
        SearchResult {
            result_type,
            id: id.to_string(),
            text: format!("Text for {id}"),
            created_at: Utc::now(),
            score,
            highlights: vec![],
            metadata: serde_json::Value::Null,
        }
    }

    fn make_semantic_hit(doc_id: &str, score: f32, doc_type: &str) -> VectorSearchResult {
        VectorSearchResult {
            doc_id: doc_id.to_string(),
            doc_type: doc_type.to_string(),
            score,
        }
    }

    #[test]
    fn test_rrf_basic() {
        let lexical = vec![
            make_lexical_hit("A", 10.0, SearchResultType::Tweet),
            make_lexical_hit("B", 8.0, SearchResultType::Tweet),
            make_lexical_hit("C", 6.0, SearchResultType::Tweet),
        ];
        let semantic = vec![
            make_semantic_hit("A", 0.9, "tweet"),
            make_semantic_hit("D", 0.8, "tweet"),
            make_semantic_hit("B", 0.7, "tweet"),
        ];

        let fused = rrf_fuse(&lexical, &semantic, 10, 0);

        // A should be first (appears in both, rank 0 in both)
        assert_eq!(fused[0].doc_id, "A");
        assert!(fused[0].in_both);

        // B should be second (appears in both, but lower combined rank)
        assert_eq!(fused[1].doc_id, "B");
        assert!(fused[1].in_both);
    }

    #[test]
    fn test_rrf_scoring() {
        let lexical = vec![make_lexical_hit("A", 10.0, SearchResultType::Tweet)]; // rank 0
        let semantic = vec![make_semantic_hit("A", 0.9, "tweet")]; // rank 0

        let fused = rrf_fuse(&lexical, &semantic, 10, 0);

        // RRF score = 1/(60+0+1) + 1/(60+0+1) = 2/61 ≈ 0.0328
        let expected_score = 2.0 / 61.0;
        assert!((fused[0].score - expected_score).abs() < 0.001);
    }

    #[test]
    fn test_rrf_single_source() {
        // Only lexical results
        let lexical = vec![
            make_lexical_hit("A", 10.0, SearchResultType::Tweet),
            make_lexical_hit("B", 8.0, SearchResultType::Tweet),
        ];
        let semantic: Vec<VectorSearchResult> = vec![];

        let fused = rrf_fuse(&lexical, &semantic, 10, 0);

        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].doc_id, "A");
        assert!(!fused[0].in_both);

        // Score should be 1/(60+0+1) = 1/61
        let expected_score = 1.0 / 61.0;
        assert!((fused[0].score - expected_score).abs() < 0.001);
    }

    #[test]
    fn test_rrf_limit() {
        let lexical = vec![
            make_lexical_hit("A", 10.0, SearchResultType::Tweet),
            make_lexical_hit("B", 8.0, SearchResultType::Tweet),
            make_lexical_hit("C", 6.0, SearchResultType::Tweet),
        ];
        let semantic: Vec<VectorSearchResult> = vec![];

        let fused = rrf_fuse(&lexical, &semantic, 2, 0);

        assert_eq!(fused.len(), 2);
    }

    #[test]
    fn test_rrf_offset() {
        let lexical = vec![
            make_lexical_hit("A", 10.0, SearchResultType::Tweet),
            make_lexical_hit("B", 8.0, SearchResultType::Tweet),
            make_lexical_hit("C", 6.0, SearchResultType::Tweet),
        ];
        let semantic: Vec<VectorSearchResult> = vec![];

        let fused = rrf_fuse(&lexical, &semantic, 10, 1);

        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].doc_id, "B"); // A is skipped
    }

    #[test]
    fn test_rrf_empty() {
        let lexical: Vec<SearchResult> = vec![];
        let semantic: Vec<VectorSearchResult> = vec![];

        let fused = rrf_fuse(&lexical, &semantic, 10, 0);

        assert!(fused.is_empty());
    }

    #[test]
    fn test_rrf_zero_limit() {
        let lexical = vec![make_lexical_hit("A", 10.0, SearchResultType::Tweet)];
        let semantic = vec![make_semantic_hit("A", 0.9, "tweet")];

        let fused = rrf_fuse(&lexical, &semantic, 0, 0);

        assert!(fused.is_empty());
    }

    #[test]
    fn test_rrf_deterministic() {
        let lexical = vec![
            make_lexical_hit("A", 5.0, SearchResultType::Tweet),
            make_lexical_hit("B", 5.0, SearchResultType::Tweet),
            make_lexical_hit("C", 5.0, SearchResultType::Tweet),
        ];
        let semantic: Vec<VectorSearchResult> = vec![];

        // Run multiple times
        let fused1 = rrf_fuse(&lexical, &semantic, 10, 0);
        let fused2 = rrf_fuse(&lexical, &semantic, 10, 0);
        let fused3 = rrf_fuse(&lexical, &semantic, 10, 0);

        // Order should be identical
        for i in 0..3 {
            assert_eq!(fused1[i].doc_id, fused2[i].doc_id);
            assert_eq!(fused2[i].doc_id, fused3[i].doc_id);
        }
    }

    #[test]
    fn test_rrf_both_bonus() {
        // Same RRF score, but "both" should rank higher
        let lexical = vec![
            make_lexical_hit("solo_lex", 10.0, SearchResultType::Tweet), // rank 0
            make_lexical_hit("both", 5.0, SearchResultType::Tweet),      // rank 1
        ];
        let semantic = vec![
            make_semantic_hit("solo_sem", 0.9, "tweet"), // rank 0
            make_semantic_hit("both", 0.5, "tweet"),     // rank 1
        ];

        let fused = rrf_fuse(&lexical, &semantic, 10, 0);

        // "both" should be first because it appears in both lists
        // and has combined score from both (even though individual ranks are lower)
        assert_eq!(fused[0].doc_id, "both");
        assert!(fused[0].in_both);
    }

    #[test]
    fn test_search_mode_parsing() {
        assert_eq!(
            "lexical".parse::<SearchMode>().unwrap(),
            SearchMode::Lexical
        );
        assert_eq!(
            "keyword".parse::<SearchMode>().unwrap(),
            SearchMode::Lexical
        );
        assert_eq!(
            "semantic".parse::<SearchMode>().unwrap(),
            SearchMode::Semantic
        );
        assert_eq!(
            "vector".parse::<SearchMode>().unwrap(),
            SearchMode::Semantic
        );
        assert_eq!("hybrid".parse::<SearchMode>().unwrap(), SearchMode::Hybrid);
        assert_eq!("rrf".parse::<SearchMode>().unwrap(), SearchMode::Hybrid);
        assert!("invalid".parse::<SearchMode>().is_err());
    }

    #[test]
    fn test_candidate_count() {
        assert_eq!(candidate_count(10, 0), 30); // 10 * 3
        assert_eq!(candidate_count(10, 5), 45); // (10 + 5) * 3
        assert_eq!(candidate_count(0, 0), 0);
    }

    #[test]
    fn test_rrf_separates_types_with_same_id() {
        let lexical = vec![
            make_lexical_hit("42", 10.0, SearchResultType::Tweet),
            make_lexical_hit("42", 9.0, SearchResultType::Like),
        ];
        let semantic = vec![make_semantic_hit("42", 0.8, "like")];

        let fused = rrf_fuse(&lexical, &semantic, 10, 0);
        let matching: Vec<_> = fused.iter().filter(|hit| hit.doc_id == "42").collect();

        assert_eq!(matching.len(), 2);
        assert!(matching.iter().any(|hit| hit.doc_type == "tweet"));
        assert!(matching.iter().any(|hit| hit.doc_type == "like"));
    }
}
