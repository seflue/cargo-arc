//! Ordering elementary cycles into display blocks (ca-0372).
//!
//! Pure and generic over the node type so the ordering can be tested without
//! any graph machinery.

/// Rotate each cycle to start at its `rank`-smallest node, then sort the
/// resulting blocks by (start rank, length, rest-sequence rank
/// lexicographically) so a reader can walk the blocks top to bottom in the
/// same order the nodes appear in the diagram.
pub fn order_cycle_blocks<N: Copy>(cycles: &[Vec<N>], rank: impl Fn(N) -> usize) -> Vec<Vec<N>> {
    let mut blocks: Vec<Vec<N>> = cycles
        .iter()
        .map(|cycle| rotate_to_min(cycle, &rank))
        .collect();
    blocks.sort_by_key(|block| {
        let rest_ranks: Vec<usize> = block[1..].iter().map(|&n| rank(n)).collect();
        (rank(block[0]), block.len(), rest_ranks)
    });
    blocks
}

/// Rotate `cycle` so it starts at its `rank`-smallest node, preserving direction.
fn rotate_to_min<N: Copy>(cycle: &[N], rank: &impl Fn(N) -> usize) -> Vec<N> {
    let min_pos = (0..cycle.len())
        .min_by_key(|&i| rank(cycle[i]))
        .unwrap_or(0);
    let mut rotated = cycle.to_vec();
    rotated.rotate_left(min_pos);
    rotated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_by_start_then_length_then_lexicographic_rest() {
        // a=0, b=1, c=2, d=3; rank is the node's own value.
        let cycles = vec![vec![0, 1, 2], vec![0, 1, 3], vec![0, 1, 2, 3]];
        let ordered = order_cycle_blocks(&cycles, |n: usize| n);
        assert_eq!(
            ordered,
            vec![vec![0, 1, 2], vec![0, 1, 3], vec![0, 1, 2, 3]]
        );
    }

    #[test]
    fn rotates_each_cycle_to_its_rank_smallest_node() {
        // c=2, a=0, b=1: rotates to start at a.
        let cycles = vec![vec![2, 0, 1]];
        let ordered = order_cycle_blocks(&cycles, |n: usize| n);
        assert_eq!(ordered, vec![vec![0, 1, 2]]);
    }
}
