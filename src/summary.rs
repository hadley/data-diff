//! Reduce changed cells to an exact minimum set of row and column edits.

use std::collections::VecDeque;

use crate::cells::{CellChanges, ColumnChanges};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SummaryChanges {
    pub optimal: bool,
    pub columns: Vec<SummaryColumn>,
    pub rows: Vec<SummaryRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SummaryColumn {
    pub old: usize,
    pub new: usize,
    pub type_changed: bool,
    /// Changed cells in this column, over the one-to-one matched rows.
    pub changes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SummaryRow {
    pub old: usize,
    pub new: usize,
    /// Changed cells in this row, over the identified columns.
    pub changes: usize,
}

/// Reduce the changed cells, holding retyped and hinted columns out of it.
///
/// `forced` names identities a valid `col_edit()` hint attached to. They join
/// the retyped columns in being column edits whatever the optimizer would have
/// preferred, which is the whole of what the hint does here: a rectangular
/// change can be described by its rows or by its columns, and the hint says
/// which. Their cells leave the graph with them, so the row edits are minimal
/// over what is left to cover rather than being computed and then overridden.
///
/// Every chosen event then counts the changed cells incident to it, over the
/// whole cell set rather than over the graph. A row edit and a column edit that
/// cross both count the cell they share, so the counts do not sum to the number
/// of changed cells: each is a fact about its own row or column, which keeps it
/// checkable against the data and independent of which tied minimum cover was
/// chosen.
pub(crate) fn summarize(changes: &CellChanges, forced: &[(usize, usize)]) -> SummaryChanges {
    let held_out =
        |column: &&ColumnChanges| column.type_changed || forced.contains(&(column.old, column.new));

    let mut columns = changes
        .columns
        .iter()
        .filter(held_out)
        .map(|column| SummaryColumn {
            old: column.old,
            new: column.new,
            type_changed: column.type_changed,
            changes: column.rows.len(),
        })
        .collect::<Vec<_>>();
    let residual_columns = changes
        .columns
        .iter()
        .filter(|column| !held_out(column) && column.values_changed())
        .collect::<Vec<_>>();

    // Each remaining cell is an edge between its matched-row identity and
    // identified-column identity. Dense stable IDs keep the solver independent
    // of Arrow and preserve deterministic output order.
    let mut rows = residual_columns
        .iter()
        .flat_map(|column| column.rows.iter().copied())
        .collect::<Vec<_>>();
    rows.sort_unstable();
    rows.dedup();
    let mut edges = Vec::new();
    for (column, changes) in residual_columns.iter().enumerate() {
        for row in &changes.rows {
            edges.push((rows.binary_search(row).unwrap(), column));
        }
    }
    let cover =
        BipartiteGraph::new(rows.len(), residual_columns.len(), &edges).minimum_vertex_cover();

    let mut selected_rows = cover
        .left
        .iter()
        .map(|&index| rows[index])
        .collect::<Vec<_>>();
    for &index in &cover.right {
        let column = residual_columns[index];
        columns.push(SummaryColumn {
            old: column.old,
            new: column.new,
            type_changed: false,
            changes: column.rows.len(),
        });
    }
    columns.sort_by_key(|column| (column.old, column.new));
    selected_rows.sort_unstable();

    // Counted over every changed column rather than over the graph, so a cell
    // in a held-out column still counts toward the row it fell in. A hint moves
    // which events are reported; it does not change what is true of a row.
    let rows = selected_rows
        .into_iter()
        .map(|row| SummaryRow {
            old: row.0,
            new: row.1,
            changes: changes
                .columns
                .iter()
                .filter(|column| column.rows.contains(&row))
                .count(),
        })
        .collect::<Vec<_>>();

    let summary = SummaryChanges {
        optimal: true,
        columns,
        rows,
    };
    debug_assert!(changes.columns.iter().all(|column| {
        column.rows.iter().all(|row| {
            summary
                .columns
                .iter()
                .any(|selected| selected.old == column.old && selected.new == column.new)
                || summary
                    .rows
                    .iter()
                    .any(|selected| (selected.old, selected.new) == *row)
        })
    }));
    summary
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BipartiteGraph {
    left_count: usize,
    right_count: usize,
    adjacency: Vec<Vec<usize>>,
}

impl BipartiteGraph {
    fn new(left_count: usize, right_count: usize, edges: &[(usize, usize)]) -> Self {
        let mut adjacency = vec![Vec::new(); left_count];
        for &(left, right) in edges {
            assert!(left < left_count);
            assert!(right < right_count);
            adjacency[left].push(right);
        }
        for neighbors in &mut adjacency {
            neighbors.sort_unstable();
            neighbors.dedup();
        }
        Self {
            left_count,
            right_count,
            adjacency,
        }
    }

    fn minimum_vertex_cover(&self) -> VertexCover {
        let matching = self.maximum_matching();
        let mut reachable_left = vec![false; self.left_count];
        let mut reachable_right = vec![false; self.right_count];
        let mut queue = VecDeque::new();

        // Follow alternating paths from unmatched left vertices: unmatched
        // edges left-to-right, then matched edges right-to-left.
        for (left, matched) in matching.left.iter().enumerate() {
            if matched.is_none() {
                reachable_left[left] = true;
                queue.push_back(left);
            }
        }
        while let Some(left) = queue.pop_front() {
            for &right in &self.adjacency[left] {
                if matching.left[left] == Some(right) || reachable_right[right] {
                    continue;
                }
                reachable_right[right] = true;
                if let Some(next_left) = matching.right[right]
                    && !reachable_left[next_left]
                {
                    reachable_left[next_left] = true;
                    queue.push_back(next_left);
                }
            }
        }

        // By König's theorem, this alternating-path partition recovers a
        // minimum vertex cover whose size equals the maximum matching.
        let cover = VertexCover {
            left: reachable_left
                .iter()
                .enumerate()
                .filter_map(|(left, reachable)| (!reachable).then_some(left))
                .collect(),
            right: reachable_right
                .iter()
                .enumerate()
                .filter_map(|(right, reachable)| reachable.then_some(right))
                .collect(),
        };
        debug_assert!(cover.covers(self));
        debug_assert_eq!(cover.len(), matching.len());
        cover
    }

    fn maximum_matching(&self) -> Matching {
        let mut matching = Matching {
            left: vec![None; self.left_count],
            right: vec![None; self.right_count],
        };
        let mut distances = vec![usize::MAX; self.left_count];

        // Hopcroft-Karp augments a complete layer of shortest paths at a time.
        while let Some(shortest) = self.layer_augmenting_paths(&matching, &mut distances) {
            for left in 0..self.left_count {
                if matching.left[left].is_none() {
                    self.augment(left, shortest, &mut distances, &mut matching);
                }
            }
        }
        matching
    }

    fn layer_augmenting_paths(
        &self,
        matching: &Matching,
        distances: &mut [usize],
    ) -> Option<usize> {
        // Breadth-first search layers matched left vertices until it reaches
        // the nearest unmatched right vertex.
        let mut queue = VecDeque::new();
        for (left, distance) in distances.iter_mut().enumerate() {
            if matching.left[left].is_none() {
                *distance = 0;
                queue.push_back(left);
            } else {
                *distance = usize::MAX;
            }
        }

        let mut shortest = usize::MAX;
        while let Some(left) = queue.pop_front() {
            if distances[left] >= shortest {
                continue;
            }
            for &right in &self.adjacency[left] {
                match matching.right[right] {
                    None => shortest = distances[left] + 1,
                    Some(next_left) if distances[next_left] == usize::MAX => {
                        distances[next_left] = distances[left] + 1;
                        queue.push_back(next_left);
                    }
                    Some(_) => {}
                }
            }
        }
        (shortest != usize::MAX).then_some(shortest)
    }

    fn augment(
        &self,
        left: usize,
        shortest: usize,
        distances: &mut [usize],
        matching: &mut Matching,
    ) -> bool {
        // Depth-first search follows only the shortest-path layers established
        // above, rewiring the matching when it reaches a free right vertex.
        for &right in &self.adjacency[left] {
            let can_augment = match matching.right[right] {
                None => distances[left] + 1 == shortest,
                Some(next_left) => {
                    distances[next_left] == distances[left] + 1
                        && self.augment(next_left, shortest, distances, matching)
                }
            };
            if can_augment {
                matching.left[left] = Some(right);
                matching.right[right] = Some(left);
                return true;
            }
        }
        distances[left] = usize::MAX;
        false
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct VertexCover {
    left: Vec<usize>,
    right: Vec<usize>,
}

impl VertexCover {
    fn len(&self) -> usize {
        self.left.len() + self.right.len()
    }

    fn covers(&self, graph: &BipartiteGraph) -> bool {
        graph.adjacency.iter().enumerate().all(|(left, rights)| {
            rights
                .iter()
                .all(|right| self.left.contains(&left) || self.right.contains(right))
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Matching {
    left: Vec<Option<usize>>,
    right: Vec<Option<usize>>,
}

impl Matching {
    fn len(&self) -> usize {
        self.left.iter().filter(|right| right.is_some()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::{BipartiteGraph, SummaryChanges, SummaryColumn, SummaryRow, VertexCover};
    use crate::cells::{CellChanges, ColumnChanges};

    fn summarize(changes: &CellChanges) -> SummaryChanges {
        super::summarize(changes, &[])
    }

    fn graph(left: usize, right: usize, edges: &[(usize, usize)]) -> BipartiteGraph {
        BipartiteGraph::new(left, right, edges)
    }

    fn is_cover(graph: &BipartiteGraph, cover: &VertexCover) -> bool {
        cover.covers(graph)
    }

    fn brute_force_optimum(graph: &BipartiteGraph) -> usize {
        let vertex_count = graph.left_count + graph.right_count;
        (0..(1_usize << vertex_count))
            .filter_map(|mask| {
                let cover = VertexCover {
                    left: (0..graph.left_count)
                        .filter(|left| mask & (1 << left) != 0)
                        .collect(),
                    right: (0..graph.right_count)
                        .filter(|right| mask & (1 << (graph.left_count + right)) != 0)
                        .collect(),
                };
                is_cover(graph, &cover).then_some(mask.count_ones() as usize)
            })
            .min()
            .unwrap()
    }

    #[test]
    fn graph_fixture_sorts_and_deduplicates_edges() {
        let graph = graph(2, 3, &[(1, 2), (0, 1), (1, 0), (1, 2)]);

        assert_eq!(graph.adjacency, [vec![1], vec![0, 2]]);
    }

    #[test]
    fn cover_assertion_detects_uncovered_edges() {
        let graph = graph(2, 2, &[(0, 0), (1, 1)]);

        assert!(is_cover(
            &graph,
            &VertexCover {
                left: vec![0],
                right: vec![1],
            }
        ));
        assert!(!is_cover(
            &graph,
            &VertexCover {
                left: vec![0],
                right: vec![],
            }
        ));
    }

    #[test]
    fn brute_force_oracle_finds_row_and_column_optima() {
        let row_star = graph(1, 3, &[(0, 0), (0, 1), (0, 2)]);
        let column_star = graph(3, 1, &[(0, 0), (1, 0), (2, 0)]);

        assert_eq!(brute_force_optimum(&row_star), 1);
        assert_eq!(brute_force_optimum(&column_star), 1);
    }

    #[test]
    fn exact_cover_handles_representative_shapes() {
        let cases = [
            (graph(0, 0, &[]), VertexCover::default()),
            (
                graph(1, 1, &[(0, 0)]),
                VertexCover {
                    left: vec![0],
                    right: vec![],
                },
            ),
            (
                graph(3, 1, &[(0, 0), (1, 0), (2, 0)]),
                VertexCover {
                    left: vec![],
                    right: vec![0],
                },
            ),
            (
                graph(1, 3, &[(0, 0), (0, 1), (0, 2)]),
                VertexCover {
                    left: vec![0],
                    right: vec![],
                },
            ),
            (
                graph(3, 3, &[(0, 0), (1, 1), (1, 2)]),
                VertexCover {
                    left: vec![0, 1],
                    right: vec![],
                },
            ),
        ];

        for (graph, expected) in cases {
            assert_eq!(graph.minimum_vertex_cover(), expected);
        }
    }

    #[test]
    fn tied_cover_is_deterministic_and_ignores_isolates() {
        let graph = graph(3, 3, &[(0, 0), (0, 1), (1, 0), (1, 1)]);
        let first = graph.minimum_vertex_cover();

        assert_eq!(
            first,
            VertexCover {
                left: vec![0, 1],
                right: vec![],
            }
        );
        assert_eq!(graph.minimum_vertex_cover(), first);
    }

    #[test]
    fn disconnected_components_choose_a_row_and_a_column() {
        let graph = graph(3, 3, &[(0, 0), (0, 1), (1, 2), (2, 2)]);

        assert_eq!(
            graph.minimum_vertex_cover(),
            VertexCover {
                left: vec![0],
                right: vec![2],
            }
        );
    }

    #[test]
    fn every_graph_through_three_by_three_is_exact_and_stable() {
        for left_count in 0..=3 {
            for right_count in 0..=3 {
                let possible_edges = left_count * right_count;
                for edge_mask in 0..(1_usize << possible_edges) {
                    let edges = (0..possible_edges)
                        .filter(|edge| edge_mask & (1 << edge) != 0)
                        .map(|edge| (edge / right_count, edge % right_count))
                        .collect::<Vec<_>>();
                    let graph = graph(left_count, right_count, &edges);
                    let cover = graph.minimum_vertex_cover();

                    assert!(is_cover(&graph, &cover));
                    assert_eq!(cover.len(), brute_force_optimum(&graph));
                    assert_eq!(graph.minimum_vertex_cover(), cover);
                }
            }
        }
    }

    #[test]
    fn forced_columns_are_coalesced_before_optimization() {
        let changes = CellChanges {
            columns: vec![
                changed_column(0, 1, true, &[(0, 1), (1, 0)]),
                changed_column(2, 0, false, &[(0, 1)]),
                changed_column(3, 3, true, &[]),
            ],
            ..CellChanges::default()
        };

        assert_eq!(
            summarize(&changes),
            super::SummaryChanges {
                optimal: true,
                columns: vec![summary_column(0, 1, true, 2), summary_column(3, 3, true, 0),],
                rows: vec![summary_row(0, 1, 2)],
            }
        );
    }

    #[test]
    fn a_forced_column_leaves_the_optimizer_and_takes_its_cells_with_it() {
        // One column changing in both rows, and each row changing in a column
        // of its own. Covering the two rows takes two vertices and covering the
        // three columns takes three, so the answer is two row edits.
        let changes = CellChanges {
            columns: vec![
                changed_column(1, 1, false, &[(0, 0), (1, 1)]),
                changed_column(2, 2, false, &[(0, 0)]),
                changed_column(3, 3, false, &[(1, 1)]),
            ],
            ..CellChanges::default()
        };

        let free = summarize(&changes);
        assert_eq!(free.rows, [summary_row(0, 0, 2), summary_row(1, 1, 2)]);
        assert!(free.columns.is_empty());

        // Hint the two single-cell columns and they leave the graph, taking
        // their cells with them. What is left to cover is one column spanning
        // both rows, so the minimum is now that column and there is no row edit
        // at all. The row summary changed because the graph did, not because
        // anything overrode the answer it produced.
        let forced = super::summarize(&changes, &[(2, 2), (3, 3)]);

        assert_eq!(
            forced.columns,
            [
                summary_column(1, 1, false, 2),
                summary_column(2, 2, false, 1),
                summary_column(3, 3, false, 1),
            ]
        );
        assert!(forced.rows.is_empty());
    }

    #[test]
    fn overlapping_events_each_count_the_cell_they_share() {
        // Five changed cells: row 0 changes in all three columns, and column 2
        // changes in all three rows. Covering them takes that row and that
        // column, and the cell where they cross belongs to both.
        let changes = CellChanges {
            columns: vec![
                changed_column(0, 0, false, &[(0, 0)]),
                changed_column(1, 1, false, &[(0, 0)]),
                changed_column(2, 2, false, &[(0, 0), (1, 1), (2, 2)]),
            ],
            ..CellChanges::default()
        };

        let summary = summarize(&changes);

        // Three and three over five cells. The counts are deliberately not a
        // partition: each is a fact about its own row or column, which is what
        // makes it checkable against the data and keeps it independent of which
        // minimum cover was chosen.
        assert_eq!(summary.columns, [summary_column(2, 2, false, 3)]);
        assert_eq!(summary.rows, [summary_row(0, 0, 3)]);
    }

    #[test]
    fn a_row_counts_cells_in_a_column_held_out_of_the_graph() {
        // "a" changes in both rows and "b" in the first, so covering the rows is
        // the smaller description until "a" is hinted out of the graph.
        let changes = CellChanges {
            columns: vec![
                changed_column(1, 1, false, &[(0, 0), (1, 1)]),
                changed_column(2, 2, false, &[(0, 0)]),
            ],
            ..CellChanges::default()
        };

        let forced = super::summarize(&changes, &[(1, 1)]);

        // Row 0 is reported for the one cell left to cover, and counts two:
        // the cell in the hinted column is still a changed cell in that row. A
        // hint moves which events are reported, not what is true of a row.
        assert_eq!(forced.columns, [summary_column(1, 1, false, 2)]);
        assert_eq!(forced.rows, [summary_row(0, 0, 2)]);
    }

    #[test]
    fn selected_vertices_retain_moved_identities() {
        let column_dominant = CellChanges {
            columns: vec![changed_column(2, 0, false, &[(0, 2), (1, 0)])],
            ..CellChanges::default()
        };
        let row_dominant = CellChanges {
            columns: vec![
                changed_column(1, 2, false, &[(0, 2)]),
                changed_column(2, 1, false, &[(0, 2)]),
            ],
            ..CellChanges::default()
        };

        assert_eq!(
            summarize(&column_dominant).columns,
            [summary_column(2, 0, false, 2)]
        );
        assert_eq!(summarize(&row_dominant).rows, [summary_row(0, 2, 2)]);
    }

    fn changed_column(
        old: usize,
        new: usize,
        type_changed: bool,
        rows: &[(usize, usize)],
    ) -> ColumnChanges {
        ColumnChanges {
            old,
            new,
            type_changed,
            rows: rows.to_vec(),
        }
    }

    fn summary_column(old: usize, new: usize, type_changed: bool, changes: usize) -> SummaryColumn {
        SummaryColumn {
            old,
            new,
            type_changed,
            changes,
        }
    }

    fn summary_row(old: usize, new: usize, changes: usize) -> SummaryRow {
        SummaryRow { old, new, changes }
    }
}
