//! Pairwise ranking: ask the cheap model "which of these two fits better?" over
//! the small refinement set, then aggregate the (possibly noisy) outcomes into a
//! calibrated order with Bradley–Terry. This is DESIGN's preferred method; it is
//! only ever run on a handful of top candidates, so the all-pairs comparison
//! count stays small.

use std::cmp::Ordering;

use crate::model::{JobText, ModelClient};

/// Iterations of the Bradley–Terry MM update. Converges quickly for the tiny
/// item counts we feed it.
const BT_ITERATIONS: usize = 50;

/// Estimates Bradley–Terry strengths from pairwise wins for `n` items.
///
/// `wins[i][j]` is the number of times item `i` beat item `j`. Returns one
/// positive strength per item, normalized to sum to `n`, via the standard
/// minorization–maximization update. Items with no comparisons keep a neutral
/// strength of 1.0.
fn bradley_terry(n: usize, wins: &[Vec<f32>]) -> Vec<f32> {
    let mut strength = vec![1.0f32; n];
    let total_wins: Vec<f32> = (0..n).map(|i| (0..n).map(|j| wins[i][j]).sum()).collect();

    for _ in 0..BT_ITERATIONS {
        let mut next = vec![0.0f32; n];
        for i in 0..n {
            let mut denominator = 0.0f32;
            for j in 0..n {
                if i == j {
                    continue;
                }
                let games = wins[i][j] + wins[j][i];
                if games > 0.0 {
                    denominator += games / (strength[i] + strength[j]);
                }
            }
            next[i] = if denominator > 0.0 {
                total_wins[i] / denominator
            } else {
                strength[i] // untouched item: keep its current strength
            };
        }
        // Normalize to keep the iteration stable (BT strengths are scale-free).
        let sum: f32 = next.iter().sum();
        if sum > 0.0 {
            let scale = n as f32 / sum;
            for s in &mut next {
                *s *= scale;
            }
        }
        strength = next;
    }
    strength
}

/// Ranks `items` by running all unique pairwise comparisons through `client` and
/// aggregating with Bradley–Terry. Returns indices into `items`, best first,
/// paired with their strength. The caller is responsible for keeping `items`
/// small (this is O(n²) model calls).
pub async fn rank<C: ModelClient + ?Sized>(
    client: &C,
    resume: &str,
    items: &[JobText],
) -> anyhow::Result<Vec<(usize, f32)>> {
    let n = items.len();
    let mut wins = vec![vec![0.0f32; n]; n];

    for i in 0..n {
        for j in (i + 1)..n {
            match client.compare(resume, &items[i], &items[j]).await? {
                Ordering::Greater => wins[i][j] += 1.0,
                Ordering::Less => wins[j][i] += 1.0,
                Ordering::Equal => {
                    // Split a tie so neither gains an edge.
                    wins[i][j] += 0.5;
                    wins[j][i] += 0.5;
                }
            }
        }
    }

    let strength = bradley_terry(n, &wins);
    let mut ranked: Vec<(usize, f32)> = (0..n).map(|i| (i, strength[i])).collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    Ok(ranked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FakeClient;

    fn job(id: &str) -> JobText {
        JobText {
            id: id.to_string(),
            title: id.to_string(),
            summary: String::new(),
        }
    }

    #[tokio::test]
    async fn recovers_a_planted_order() {
        // Planted strengths: c > a > d > b. Order should come back regardless of
        // input order.
        let client = FakeClient::new([("a", 3.0), ("b", 1.0), ("c", 4.0), ("d", 2.0)]);
        let items = [job("a"), job("b"), job("c"), job("d")];

        let ranked = rank(&client, "resume", &items).await.expect("rank");
        let order: Vec<&str> = ranked.iter().map(|(i, _)| items[*i].id.as_str()).collect();

        assert_eq!(order, ["c", "a", "d", "b"]);
    }

    #[tokio::test]
    async fn single_item_is_trivially_ranked() {
        let client = FakeClient::new([("only", 1.0)]);
        let items = [job("only")];

        let ranked = rank(&client, "resume", &items).await.expect("rank");
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].0, 0);
    }
}
