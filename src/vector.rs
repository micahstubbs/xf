//! Vector search for semantic similarity.
//!
//! Provides in-memory vector search with SIMD-accelerated dot product.
//! Vectors are loaded from `SQLite` at startup and searched using cosine
//! similarity (which equals dot product for L2-normalized vectors).

use crate::embedder::dot_product_simd;
use crate::storage::Storage;
use anyhow::Result;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Result of a vector search.
#[derive(Debug, Clone)]
pub struct VectorSearchResult {
    /// Document ID.
    pub doc_id: String,
    /// Document type (tweet, like, dm, grok).
    pub doc_type: String,
    /// Similarity score (cosine similarity, range -1.0 to 1.0).
    pub score: f32,
}

/// Entry in the min-heap for top-k selection.
#[derive(Debug, Clone)]
struct ScoredEntry {
    score: f32,
    doc_id: String,
    doc_type: String,
}

impl PartialEq for ScoredEntry {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score && self.doc_id == other.doc_id
    }
}

impl Eq for ScoredEntry {}

impl PartialOrd for ScoredEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap: lower score is "greater" (gets popped first)
        // This way we keep the highest scores in the heap
        other
            .score
            .total_cmp(&self.score)
            .then_with(|| self.doc_id.cmp(&other.doc_id))
    }
}

/// In-memory vector index for fast similarity search.
pub struct VectorIndex {
    /// All stored vectors with their metadata.
    vectors: Vec<(String, String, Vec<f32>)>, // (doc_id, doc_type, embedding)
    /// Embedding dimension.
    dimension: usize,
}

impl VectorIndex {
    /// Create a new empty vector index.
    #[must_use]
    pub const fn new(dimension: usize) -> Self {
        Self {
            vectors: Vec::new(),
            dimension,
        }
    }

    /// Load all embeddings from storage.
    ///
    /// # Errors
    ///
    /// Returns an error if loading from storage fails.
    pub fn load_from_storage(storage: &Storage) -> Result<Self> {
        let embeddings = storage.load_all_embeddings()?;

        let dimension = embeddings.first().map_or(384, |(_, _, v)| v.len());

        Ok(Self {
            vectors: embeddings,
            dimension,
        })
    }

    /// Add a vector to the index.
    pub fn add(&mut self, doc_id: String, doc_type: String, embedding: Vec<f32>) {
        debug_assert_eq!(
            embedding.len(),
            self.dimension,
            "embedding dimension mismatch"
        );
        self.vectors.push((doc_id, doc_type, embedding));
    }

    /// Get the number of vectors in the index.
    #[must_use]
    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    /// Check if the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    /// Get the embedding dimension.
    #[must_use]
    pub const fn dimension(&self) -> usize {
        self.dimension
    }

    /// Search for the top-k most similar vectors.
    ///
    /// Uses SIMD-accelerated dot product for fast computation.
    /// Results are sorted by score (descending), then by `doc_id` (ascending)
    /// for deterministic ordering.
    #[must_use]
    pub fn search_top_k(
        &self,
        query: &[f32],
        k: usize,
        doc_types: Option<&[&str]>,
    ) -> Vec<VectorSearchResult> {
        if k == 0 || self.is_empty() || query.len() != self.dimension {
            return Vec::new();
        }

        // Use a min-heap to keep track of top-k
        let mut heap: BinaryHeap<ScoredEntry> = BinaryHeap::with_capacity(k + 1);

        for (doc_id, doc_type, embedding) in &self.vectors {
            // Filter by doc_type if specified
            if let Some(types) = doc_types {
                if !types.contains(&doc_type.as_str()) {
                    continue;
                }
            }

            // Compute similarity using SIMD dot product
            let score = dot_product_simd(query, embedding);

            heap.push(ScoredEntry {
                score,
                doc_id: doc_id.clone(),
                doc_type: doc_type.clone(),
            });

            // Keep only top-k by removing the minimum when heap exceeds k
            if heap.len() > k {
                heap.pop();
            }
        }

        // Convert to results and sort by score descending
        let mut results: Vec<VectorSearchResult> = heap
            .into_iter()
            .map(|entry| VectorSearchResult {
                doc_id: entry.doc_id,
                doc_type: entry.doc_type,
                score: entry.score,
            })
            .collect();

        // Sort by score descending, then doc_id ascending for determinism
        results.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.doc_id.cmp(&b.doc_id))
        });

        results
    }

    /// Search with parallel processing for large indices.
    ///
    /// Uses rayon to parallelize the search across multiple CPU cores.
    /// Falls back to sequential search for small indices.
    #[cfg(feature = "parallel-search")]
    pub fn search_top_k_parallel(
        &self,
        query: &[f32],
        k: usize,
        doc_types: Option<&[&str]>,
    ) -> Vec<VectorSearchResult> {
        use rayon::prelude::*;

        const PARALLEL_THRESHOLD: usize = 10_000;
        const CHUNK_SIZE: usize = 1024;

        if self.vectors.len() < PARALLEL_THRESHOLD {
            return self.search_top_k(query, k, doc_types);
        }

        // Parallel scan with thread-local heaps
        let partial_results: Vec<Vec<ScoredEntry>> = self
            .vectors
            .par_chunks(CHUNK_SIZE)
            .map(|chunk| {
                let mut local_heap = BinaryHeap::with_capacity(k + 1);

                for (doc_id, doc_type, embedding) in chunk {
                    if let Some(types) = doc_types {
                        if !types.contains(&doc_type.as_str()) {
                            continue;
                        }
                    }

                    let score = dot_product_simd(query, embedding);

                    local_heap.push(ScoredEntry {
                        score,
                        doc_id: doc_id.clone(),
                        doc_type: doc_type.clone(),
                    });

                    if local_heap.len() > k {
                        local_heap.pop();
                    }
                }

                local_heap.into_vec()
            })
            .collect();

        // Merge thread-local results
        let mut final_heap = BinaryHeap::with_capacity(k + 1);
        for entries in partial_results {
            for entry in entries {
                final_heap.push(entry);
                if final_heap.len() > k {
                    final_heap.pop();
                }
            }
        }

        let mut results: Vec<VectorSearchResult> = final_heap
            .into_iter()
            .map(|entry| VectorSearchResult {
                doc_id: entry.doc_id,
                doc_type: entry.doc_type,
                score: entry.score,
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.doc_id.cmp(&b.doc_id))
        });

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedder::l2_normalize;

    #[allow(clippy::cast_precision_loss)]
    fn create_test_vector(seed: u32, dim: usize) -> Vec<f32> {
        let mut vec: Vec<f32> = (0..dim)
            .map(|i| ((seed as usize * 17 + i * 13) % 100) as f32 / 100.0)
            .collect();
        l2_normalize(&mut vec);
        vec
    }

    #[test]
    fn test_new_index() {
        let index = VectorIndex::new(384);
        assert!(index.is_empty());
        assert_eq!(index.dimension(), 384);
    }

    #[test]
    fn test_add_and_len() {
        let mut index = VectorIndex::new(4);
        assert_eq!(index.len(), 0);

        let mut v1 = vec![1.0, 0.0, 0.0, 0.0];
        l2_normalize(&mut v1);
        index.add("doc1".to_string(), "tweet".to_string(), v1);

        assert_eq!(index.len(), 1);
        assert!(!index.is_empty());
    }

    #[test]
    fn test_search_empty() {
        let index = VectorIndex::new(4);
        let query = vec![1.0, 0.0, 0.0, 0.0];
        let results = index.search_top_k(&query, 10, None);
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_basic() {
        let mut index = VectorIndex::new(4);

        // Add some test vectors
        let mut v1 = vec![1.0, 0.0, 0.0, 0.0];
        let mut v2 = vec![0.9, 0.1, 0.0, 0.0];
        let mut v3 = vec![0.0, 1.0, 0.0, 0.0];

        l2_normalize(&mut v1);
        l2_normalize(&mut v2);
        l2_normalize(&mut v3);

        index.add("doc1".to_string(), "tweet".to_string(), v1.clone());
        index.add("doc2".to_string(), "tweet".to_string(), v2);
        index.add("doc3".to_string(), "like".to_string(), v3);

        // Search with query similar to v1
        let results = index.search_top_k(&v1, 2, None);

        assert_eq!(results.len(), 2);
        // doc1 should be first (exact match)
        assert_eq!(results[0].doc_id, "doc1");
        assert!((results[0].score - 1.0).abs() < 0.001);
        // doc2 should be second (similar)
        assert_eq!(results[1].doc_id, "doc2");
    }

    #[test]
    fn test_search_with_type_filter() {
        let mut index = VectorIndex::new(4);

        let mut v1 = vec![1.0, 0.0, 0.0, 0.0];
        let mut v2 = vec![0.9, 0.1, 0.0, 0.0];
        let mut v3 = vec![0.8, 0.2, 0.0, 0.0];

        l2_normalize(&mut v1);
        l2_normalize(&mut v2);
        l2_normalize(&mut v3);

        index.add("doc1".to_string(), "tweet".to_string(), v1.clone());
        index.add("doc2".to_string(), "like".to_string(), v2);
        index.add("doc3".to_string(), "dm".to_string(), v3);

        // Search only tweets
        let results = index.search_top_k(&v1, 10, Some(&["tweet"]));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].doc_id, "doc1");

        // Search tweets and likes
        let results = index.search_top_k(&v1, 10, Some(&["tweet", "like"]));
        assert_eq!(results.len(), 2);
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn test_search_limit() {
        let mut index = VectorIndex::new(4);

        // Add 10 vectors
        for i in 0..10 {
            let mut v = vec![1.0, (i as f32) / 10.0, 0.0, 0.0];
            l2_normalize(&mut v);
            index.add(format!("doc{i}"), "tweet".to_string(), v);
        }

        let query = vec![1.0, 0.0, 0.0, 0.0];
        let results = index.search_top_k(&query, 3, None);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_search_deterministic() {
        let mut index = VectorIndex::new(8);

        // Add vectors with varying similarities
        for i in 0..20 {
            index.add(
                format!("doc{i}"),
                "tweet".to_string(),
                create_test_vector(i, 8),
            );
        }

        let query = create_test_vector(100, 8);

        // Run search multiple times
        let results1 = index.search_top_k(&query, 5, None);
        let results2 = index.search_top_k(&query, 5, None);
        let results3 = index.search_top_k(&query, 5, None);

        // Results should be identical
        for i in 0..5 {
            assert_eq!(results1[i].doc_id, results2[i].doc_id);
            assert_eq!(results2[i].doc_id, results3[i].doc_id);
        }
    }

    #[test]
    fn test_search_scores_descending() {
        let mut index = VectorIndex::new(4);

        let mut v1 = vec![1.0, 0.0, 0.0, 0.0];
        let mut v2 = vec![0.7, 0.3, 0.0, 0.0];
        let mut v3 = vec![0.5, 0.5, 0.0, 0.0];

        l2_normalize(&mut v1);
        l2_normalize(&mut v2);
        l2_normalize(&mut v3);

        index.add("doc1".to_string(), "tweet".to_string(), v1.clone());
        index.add("doc2".to_string(), "tweet".to_string(), v2);
        index.add("doc3".to_string(), "tweet".to_string(), v3);

        let results = index.search_top_k(&v1, 10, None);

        // Verify scores are in descending order
        for i in 1..results.len() {
            assert!(
                results[i - 1].score >= results[i].score,
                "Results not sorted by score"
            );
        }
    }

    #[test]
    fn test_zero_k() {
        let mut index = VectorIndex::new(4);
        let mut v = vec![1.0, 0.0, 0.0, 0.0];
        l2_normalize(&mut v);
        index.add("doc1".to_string(), "tweet".to_string(), v.clone());

        let results = index.search_top_k(&v, 0, None);
        assert!(results.is_empty());
    }

    #[test]
    fn test_dimension_mismatch() {
        let mut index = VectorIndex::new(4);
        let mut v = vec![1.0, 0.0, 0.0, 0.0];
        l2_normalize(&mut v);
        index.add("doc1".to_string(), "tweet".to_string(), v);

        // Query with wrong dimension
        let wrong_query = vec![1.0, 0.0, 0.0, 0.0, 0.0]; // 5 dimensions
        let results = index.search_top_k(&wrong_query, 10, None);
        assert!(results.is_empty());
    }
}
