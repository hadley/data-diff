use crate::rows::RowMatches;
use crate::schema::ColumnMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct OrderMatches {
    pub columns: Vec<(usize, usize)>,
    pub rows: Vec<(usize, usize)>,
}

pub(crate) fn detect_order(map: &ColumnMap, rows: &RowMatches) -> OrderMatches {
    let columns = map
        .pairs()
        .iter()
        .map(|pair| (pair.old, pair.new))
        .collect::<Vec<_>>();
    OrderMatches {
        columns: minimal_moves(&columns),
        rows: minimal_moves(&rows.matched),
    }
}

/// Return the minimum identities that must move to explain relative order.
fn minimal_moves(identities: &[(usize, usize)]) -> Vec<(usize, usize)> {
    if identities.is_empty() {
        return Vec::new();
    }
    debug_assert!(identities.windows(2).all(|pair| pair[0].0 < pair[1].0));

    let new_positions = identities.iter().map(|(_, new)| *new).collect::<Vec<_>>();
    // Already in order — the commonest case of all — means nothing moved, and
    // the answer is known before any tree is built.
    if new_positions.is_sorted() {
        return Vec::new();
    }
    let mut ranks = new_positions.clone();
    ranks.sort_unstable();
    ranks.dedup();

    // Length of the best increasing continuation beginning at each identity.
    let mut suffix = vec![0; identities.len()];
    let mut tree = FenwickMax::new(ranks.len());
    for index in (0..identities.len()).rev() {
        let rank = ranks.binary_search(&new_positions[index]).unwrap();
        let reversed = ranks.len() - rank;
        suffix[index] = 1 + tree.query(reversed - 1);
        tree.update(reversed, suffix[index]);
    }

    // Choose the earliest old position that can still complete a maximum LIS.
    let mut remaining = suffix.iter().copied().max().unwrap();
    let mut last_new = None;
    let mut retained = vec![false; identities.len()];
    for (index, new) in new_positions.iter().copied().enumerate() {
        let follows = last_new.is_none_or(|last| new > last);
        if follows && suffix[index] >= remaining {
            retained[index] = true;
            last_new = Some(new);
            remaining -= 1;
            if remaining == 0 {
                break;
            }
        }
    }

    identities
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, identity)| (!retained[index]).then_some(identity))
        .collect()
}

struct FenwickMax(Vec<usize>);

impl FenwickMax {
    fn new(size: usize) -> Self {
        Self(vec![0; size + 1])
    }

    fn update(&mut self, mut index: usize, value: usize) {
        while index < self.0.len() {
            self.0[index] = self.0[index].max(value);
            index += index & index.wrapping_neg();
        }
    }

    fn query(&self, mut index: usize) -> usize {
        let mut value = 0;
        while index > 0 {
            value = value.max(self.0[index]);
            index &= index - 1;
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::minimal_moves;

    #[test]
    fn already_ordered_identities_do_not_move() {
        assert_eq!(minimal_moves(&[(0, 1), (1, 2), (2, 4)]), []);
    }

    #[test]
    fn additions_and_deletions_do_not_create_moves() {
        assert_eq!(minimal_moves(&[(0, 1), (2, 3), (4, 8)]), []);
    }

    #[test]
    fn rotation_reports_only_the_rotated_identity() {
        assert_eq!(minimal_moves(&[(0, 1), (1, 2), (2, 0)]), [(2, 0)]);
    }

    #[test]
    fn ties_retain_lexicographically_earliest_old_positions() {
        assert_eq!(minimal_moves(&[(0, 1), (2, 0)]), [(2, 0)]);
        assert_eq!(minimal_moves(&[(0, 2), (1, 1), (2, 0)]), [(1, 1), (2, 0)]);
    }

    #[test]
    fn reports_a_minimum_moved_set_for_mixed_order() {
        let identities = [(0, 3), (1, 0), (2, 1), (3, 4), (4, 2)];
        let moved = minimal_moves(&identities);

        assert_eq!(moved, [(0, 3), (4, 2)]);
    }
}
