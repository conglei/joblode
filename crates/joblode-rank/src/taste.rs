//! The keyless ranker: a "taste direction" learned from labeled embedding
//! examples (Rocchio relevance feedback). This is the token-economic backbone —
//! it scores an entire candidate set with plain vector math, no model calls, so
//! the server can reduce hundreds of matches to a short list for free.
//!
//! Embeddings already live in the dataset, and a user's liked/disliked roles are
//! just points in that space, so feedback *is* the training signal: positives
//! pull the direction toward what they liked, negatives push it away.

/// Dot product of two equal-length vectors.
fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// L2 norm.
fn norm(v: &[f32]) -> f32 {
    dot(v, v).sqrt()
}

/// Mean of a set of equal-length vectors. `None` if empty or if the vectors
/// disagree on length — failing closed rather than silently truncating (a NULL
/// embedding arrives as an empty vector, which must not bias the direction).
fn mean(vectors: &[Vec<f32>]) -> Option<Vec<f32>> {
    let first = vectors.first()?;
    let dims = first.len();
    if dims == 0 || vectors.iter().any(|v| v.len() != dims) {
        return None;
    }
    let mut acc = vec![0.0f32; dims];
    for v in vectors {
        for (a, x) in acc.iter_mut().zip(v) {
            *a += x;
        }
    }
    let n = vectors.len() as f32;
    for a in &mut acc {
        *a /= n;
    }
    Some(acc)
}

/// How hard negatives push the direction away from disliked examples (Rocchio β,
/// with α fixed at 1.0). Below 1.0 so a single dislike can't fully cancel likes.
const NEGATIVE_WEIGHT: f32 = 0.5;

/// A unit taste direction in embedding space. Score = cosine similarity, so it is
/// scale-invariant to the candidate's embedding magnitude.
#[derive(Debug, Clone)]
pub struct TasteModel {
    direction: Vec<f32>,
}

impl TasteModel {
    /// Learns a direction from positive and negative example embeddings via
    /// Rocchio: `normalize(mean(positives) - β·mean(negatives))`.
    ///
    /// Returns `None` when there is no positive signal (nothing to rank toward)
    /// or the resulting direction is degenerate (e.g. positives and negatives
    /// cancel exactly), so callers can fall back to unranked order.
    #[must_use]
    pub fn from_examples(positives: &[Vec<f32>], negatives: &[Vec<f32>]) -> Option<Self> {
        let pos = mean(positives)?;
        let mut direction = pos;
        if let Some(neg) = mean(negatives) {
            for (d, n) in direction.iter_mut().zip(&neg) {
                *d -= NEGATIVE_WEIGHT * n;
            }
        }

        let magnitude = norm(&direction);
        if magnitude == 0.0 || !magnitude.is_finite() {
            return None;
        }
        for d in &mut direction {
            *d /= magnitude;
        }
        Some(Self { direction })
    }

    /// Cosine similarity of `embedding` to the taste direction, in `[-1, 1]`.
    /// A zero or mismatched-length embedding scores 0.0.
    #[must_use]
    pub fn score(&self, embedding: &[f32]) -> f32 {
        if embedding.len() != self.direction.len() {
            return 0.0;
        }
        let magnitude = norm(embedding);
        if magnitude == 0.0 {
            return 0.0;
        }
        dot(&self.direction, embedding) / magnitude
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Two well-separated regions of a tiny embedding space.
    const A1: [f32; 3] = [1.0, 0.0, 0.0];
    const A2: [f32; 3] = [0.9, 0.1, 0.0];
    const B1: [f32; 3] = [0.0, 1.0, 0.0];
    const B2: [f32; 3] = [0.0, 0.9, 0.1];

    #[test]
    fn ranks_candidates_toward_positive_examples() {
        let model = TasteModel::from_examples(&[A1.to_vec()], &[]).expect("has a positive");

        // A-like candidates should outscore B-like ones.
        assert!(model.score(&A2) > model.score(&B1));
    }

    #[test]
    fn negatives_push_the_direction_away() {
        // Like A, dislike B: a B-leaning candidate should score below neutral.
        let model =
            TasteModel::from_examples(&[A1.to_vec()], &[B1.to_vec()]).expect("has a positive");

        assert!(model.score(&A2) > model.score(&B2));
        assert!(model.score(&B2) < 0.0, "disliked region should go negative");
    }

    #[test]
    fn feedback_changes_the_order() {
        // The same two candidates flip order when the feedback flips.
        let likes_a = TasteModel::from_examples(&[A1.to_vec()], &[B1.to_vec()]).unwrap();
        let likes_b = TasteModel::from_examples(&[B1.to_vec()], &[A1.to_vec()]).unwrap();

        assert!(likes_a.score(&A2) > likes_a.score(&B2));
        assert!(likes_b.score(&B2) > likes_b.score(&A2));
    }

    #[test]
    fn no_positive_signal_yields_no_model() {
        assert!(TasteModel::from_examples(&[], &[B1.to_vec()]).is_none());
        assert!(TasteModel::from_examples(&[], &[]).is_none());
    }

    #[test]
    fn mismatched_or_zero_embeddings_score_zero() {
        let model = TasteModel::from_examples(&[A1.to_vec()], &[]).unwrap();
        assert_eq!(model.score(&[0.0, 0.0, 0.0]), 0.0);
        assert_eq!(model.score(&[1.0, 0.0]), 0.0); // wrong length
    }

    #[test]
    fn mismatched_training_dimensions_yield_no_model() {
        // A NULL embedding (empty vec) mixed with real ones must fail closed, not
        // silently truncate the learned direction.
        assert!(TasteModel::from_examples(&[vec![1.0, 0.0, 0.0], vec![]], &[]).is_none());
        assert!(TasteModel::from_examples(&[vec![1.0, 0.0], vec![1.0]], &[]).is_none());
    }
}
