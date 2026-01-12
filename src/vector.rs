//! Vector search for semantic similarity.
//!
//! Provides in-memory vector search with SIMD-accelerated dot product.
//! Vectors are loaded from `SQLite` at startup and searched using cosine
//! similarity (which equals dot product for L2-normalized vectors).

use crate::embedder::dot_product_simd;
use crate::storage::Storage;
use anyhow::{Result, ensure};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::str;

#[allow(dead_code)]
const VECTOR_INDEX_MAGIC: [u8; 4] = *b"XFVI";
#[allow(dead_code)]
const VECTOR_INDEX_VERSION: u16 = 1;
#[allow(dead_code)]
const VECTOR_INDEX_HEADER_LEN: usize = 32;
#[allow(dead_code)]
const VECTOR_INDEX_DOC_TYPE_ENCODING: u8 = 0;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
struct VectorIndexHeader {
    version: u16,
    doc_type_encoding: u8,
    dimension: u32,
    record_count: u64,
    offsets_start: u64,
}

#[allow(dead_code)]
fn parse_vector_index_header(bytes: &[u8]) -> Result<VectorIndexHeader> {
    ensure!(
        bytes.len() >= VECTOR_INDEX_HEADER_LEN,
        "vector index header truncated"
    );
    ensure!(
        bytes[0..4] == VECTOR_INDEX_MAGIC,
        "vector index magic mismatch"
    );

    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    let doc_type_encoding = bytes[6];
    let dimension = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let record_count = u64::from_le_bytes([
        bytes[12], bytes[13], bytes[14], bytes[15], bytes[16], bytes[17], bytes[18], bytes[19],
    ]);
    let offsets_start = u64::from_le_bytes([
        bytes[20], bytes[21], bytes[22], bytes[23], bytes[24], bytes[25], bytes[26], bytes[27],
    ]);

    Ok(VectorIndexHeader {
        version,
        doc_type_encoding,
        dimension,
        record_count,
        offsets_start,
    })
}

#[allow(dead_code)]
fn validate_doc_type(value: u8) -> Result<()> {
    ensure!(value <= 3, "invalid doc_type encoding");
    Ok(())
}

#[allow(dead_code)]
fn validate_vector_index_layout(bytes: &[u8]) -> Result<()> {
    let header = parse_vector_index_header(bytes)?;

    ensure!(
        header.version == VECTOR_INDEX_VERSION,
        "unsupported vector index version"
    );
    ensure!(
        header.doc_type_encoding == VECTOR_INDEX_DOC_TYPE_ENCODING,
        "unsupported doc_type encoding"
    );
    ensure!(header.dimension > 0, "embedding dimension must be non-zero");
    let offsets_start = usize::try_from(header.offsets_start)
        .map_err(|_| anyhow::anyhow!("offsets start overflow"))?;
    ensure!(
        offsets_start >= VECTOR_INDEX_HEADER_LEN,
        "offsets start precedes header"
    );

    let offsets_bytes = header
        .record_count
        .checked_mul(8)
        .ok_or_else(|| anyhow::anyhow!("offset table size overflow"))?;
    let offsets_end = header
        .offsets_start
        .checked_add(offsets_bytes)
        .ok_or_else(|| anyhow::anyhow!("offset table end overflow"))?;
    let offsets_end =
        usize::try_from(offsets_end).map_err(|_| anyhow::anyhow!("offset table end overflow"))?;
    ensure!(
        offsets_end <= bytes.len(),
        "offset table exceeds file length"
    );

    let offsets_slice = &bytes[offsets_start..offsets_end];
    let record_base = offsets_end;
    let embedding_len = header.dimension as usize * 2;

    let mut last_offset = None;
    for chunk in offsets_slice.chunks_exact(8) {
        let offset = usize::try_from(u64::from_le_bytes([
            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ]))
        .map_err(|_| anyhow::anyhow!("record offset overflow"))?;

        ensure!(offset >= record_base, "record offset precedes data section");
        ensure!(offset < bytes.len(), "record offset out of bounds");
        if let Some(prev) = last_offset {
            ensure!(offset >= prev, "record offsets not sorted");
        }
        last_offset = Some(offset);

        let record = &bytes[offset..];
        ensure!(record.len() >= 4, "record truncated");
        let doc_type = record[0];
        validate_doc_type(doc_type)?;
        let doc_id_len = u16::from_le_bytes([record[2], record[3]]) as usize;
        ensure!(doc_id_len > 0, "doc_id length must be non-zero");

        let header_len = 4usize;
        let total_len = header_len
            .checked_add(doc_id_len)
            .and_then(|v| v.checked_add(embedding_len))
            .ok_or_else(|| anyhow::anyhow!("record length overflow"))?;
        ensure!(record.len() >= total_len, "record length exceeds file");

        let doc_id_bytes = &record[header_len..header_len + doc_id_len];
        str::from_utf8(doc_id_bytes).map_err(|_| anyhow::anyhow!("doc_id is not valid UTF-8"))?;
    }

    Ok(())
}

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
        self.score == other.score && self.doc_id == other.doc_id && self.doc_type == other.doc_type
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
            .then_with(|| self.doc_type.cmp(&other.doc_type))
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

        // Sort by score descending, then doc_id + doc_type ascending for determinism
        results.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.doc_id.cmp(&b.doc_id))
                .then_with(|| a.doc_type.cmp(&b.doc_type))
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
                .then_with(|| a.doc_type.cmp(&b.doc_type))
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

    fn build_vector_index_bytes(dimension: u32, records: &[(u8, &str)]) -> Vec<u8> {
        let offsets_start = VECTOR_INDEX_HEADER_LEN as u64;
        let offsets_len = records.len() * 8;
        let records_start = offsets_start + offsets_len as u64;

        let mut offsets: Vec<u64> = Vec::with_capacity(records.len());
        let mut record_bytes: Vec<u8> = Vec::new();
        let mut current_offset = records_start;

        for (doc_type, doc_id) in records {
            offsets.push(current_offset);
            let doc_id_bytes = doc_id.as_bytes();
            let doc_id_len = u16::try_from(doc_id_bytes.len()).expect("doc_id length fits u16");
            let embedding_len = dimension as usize * 2;

            record_bytes.push(*doc_type);
            record_bytes.push(0);
            record_bytes.extend_from_slice(&doc_id_len.to_le_bytes());
            record_bytes.extend_from_slice(doc_id_bytes);
            record_bytes.extend(std::iter::repeat_n(0u8, embedding_len));

            let record_len = 4 + doc_id_bytes.len() + embedding_len;
            current_offset = current_offset
                .checked_add(record_len as u64)
                .expect("record offset overflow");
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&VECTOR_INDEX_MAGIC);
        bytes.extend_from_slice(&VECTOR_INDEX_VERSION.to_le_bytes());
        bytes.push(VECTOR_INDEX_DOC_TYPE_ENCODING);
        bytes.push(0);
        bytes.extend_from_slice(&dimension.to_le_bytes());
        bytes.extend_from_slice(&(records.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&offsets_start.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());

        for offset in offsets {
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
        bytes.extend_from_slice(&record_bytes);

        bytes
    }

    #[test]
    fn test_vector_index_layout_validation_ok() {
        let bytes = build_vector_index_bytes(3, &[(0, "doc1"), (1, "doc2")]);
        validate_vector_index_layout(&bytes).unwrap();
    }

    #[test]
    fn test_vector_index_layout_invalid_magic() {
        let mut bytes = build_vector_index_bytes(3, &[(0, "doc1")]);
        bytes[0] = b'Z';
        assert!(validate_vector_index_layout(&bytes).is_err());
    }

    #[test]
    fn test_vector_index_layout_invalid_offsets() {
        let mut bytes = build_vector_index_bytes(3, &[(0, "doc1")]);
        let offsets_start = u64::from_le_bytes([
            bytes[20], bytes[21], bytes[22], bytes[23], bytes[24], bytes[25], bytes[26], bytes[27],
        ]);
        let bad_offset = offsets_start + 8 + 10_000;
        let start = VECTOR_INDEX_HEADER_LEN;
        bytes[start..start + 8].copy_from_slice(&bad_offset.to_le_bytes());
        assert!(validate_vector_index_layout(&bytes).is_err());
    }

    #[test]
    fn test_vector_index_layout_invalid_doc_type() {
        let mut bytes = build_vector_index_bytes(3, &[(0, "doc1")]);
        let record_offset = usize::try_from(u64::from_le_bytes([
            bytes[VECTOR_INDEX_HEADER_LEN],
            bytes[VECTOR_INDEX_HEADER_LEN + 1],
            bytes[VECTOR_INDEX_HEADER_LEN + 2],
            bytes[VECTOR_INDEX_HEADER_LEN + 3],
            bytes[VECTOR_INDEX_HEADER_LEN + 4],
            bytes[VECTOR_INDEX_HEADER_LEN + 5],
            bytes[VECTOR_INDEX_HEADER_LEN + 6],
            bytes[VECTOR_INDEX_HEADER_LEN + 7],
        ]))
        .unwrap();
        bytes[record_offset] = 9;
        assert!(validate_vector_index_layout(&bytes).is_err());
    }

    #[test]
    fn test_vector_index_layout_invalid_doc_id_len() {
        let mut bytes = build_vector_index_bytes(3, &[(0, "doc1")]);
        let record_offset = usize::try_from(u64::from_le_bytes([
            bytes[VECTOR_INDEX_HEADER_LEN],
            bytes[VECTOR_INDEX_HEADER_LEN + 1],
            bytes[VECTOR_INDEX_HEADER_LEN + 2],
            bytes[VECTOR_INDEX_HEADER_LEN + 3],
            bytes[VECTOR_INDEX_HEADER_LEN + 4],
            bytes[VECTOR_INDEX_HEADER_LEN + 5],
            bytes[VECTOR_INDEX_HEADER_LEN + 6],
            bytes[VECTOR_INDEX_HEADER_LEN + 7],
        ]))
        .unwrap();
        let doc_id_len_offset = record_offset + 2;
        bytes[doc_id_len_offset..doc_id_len_offset + 2].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(validate_vector_index_layout(&bytes).is_err());
    }

    #[test]
    fn test_vector_index_layout_invalid_utf8() {
        let mut bytes = build_vector_index_bytes(3, &[(0, "doc1")]);
        let record_offset = usize::try_from(u64::from_le_bytes([
            bytes[VECTOR_INDEX_HEADER_LEN],
            bytes[VECTOR_INDEX_HEADER_LEN + 1],
            bytes[VECTOR_INDEX_HEADER_LEN + 2],
            bytes[VECTOR_INDEX_HEADER_LEN + 3],
            bytes[VECTOR_INDEX_HEADER_LEN + 4],
            bytes[VECTOR_INDEX_HEADER_LEN + 5],
            bytes[VECTOR_INDEX_HEADER_LEN + 6],
            bytes[VECTOR_INDEX_HEADER_LEN + 7],
        ]))
        .unwrap();
        let doc_id_len =
            u16::from_le_bytes([bytes[record_offset + 2], bytes[record_offset + 3]]) as usize;
        let doc_id_offset = record_offset + 4;
        if doc_id_len > 0 {
            bytes[doc_id_offset] = 0xFF;
        }
        assert!(validate_vector_index_layout(&bytes).is_err());
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
    fn test_search_deterministic_with_same_id_types() {
        let mut index = VectorIndex::new(4);
        let mut v = vec![1.0, 0.0, 0.0, 0.0];
        l2_normalize(&mut v);

        index.add("same".to_string(), "tweet".to_string(), v.clone());
        index.add("same".to_string(), "like".to_string(), v.clone());

        let results = index.search_top_k(&v, 2, None);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].doc_id, "same");
        assert_eq!(results[1].doc_id, "same");
        assert_eq!(results[0].doc_type, "like");
        assert_eq!(results[1].doc_type, "tweet");
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
