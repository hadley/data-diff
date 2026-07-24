//! Reduce changed cells to an exact minimum set of row and column edits.

use std::collections::VecDeque;

use crate::cells::{CellChanges, ChangedColumn};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SummaryChanges {
    pub optimal: bool,
    pub columns: Vec<ChangedColumn>,
    pub rows: Vec<(usize, usize)>,
}

pub(crate) fn summarize(changes: &CellChanges) -> SummaryChanges {
    // Type changes must remain column edits. Remove their incident cells before
    // optimization, while recording whether each forced edit also changed values.
    let mut columns = changes
        .columns
        .iter()
        .filter(|column| column.type_changed)
        .map(|column| {
            let mut column = column.clone();
            column.values_changed = changes
                .cells
                .iter()
                .any(|cell| cell.old_column == column.old && cell.new_column == column.new);
            column
        })
        .collect::<Vec<_>>();
    let forced = columns
        .iter()
        .map(|column| (column.old, column.new))
        .collect::<Vec<_>>();
    let residual = changes
        .cells
        .iter()
        .filter(|cell| !forced.contains(&(cell.old_column, cell.new_column)))
        .collect::<Vec<_>>();

    // Each remaining cell is an edge between its matched-row identity and
    // identified-column identity. Dense stable IDs keep the solver independent
    // of Arrow and preserve deterministic output order.
    let mut rows = residual
        .iter()
        .map(|cell| (cell.old_row, cell.new_row))
        .collect::<Vec<_>>();
    rows.sort_unstable();
    rows.dedup();
    let mut residual_columns = residual
        .iter()
        .map(|cell| (cell.old_column, cell.new_column))
        .collect::<Vec<_>>();
    residual_columns.sort_unstable();
    residual_columns.dedup();
    let edges = residual
        .iter()
        .map(|cell| {
            (
                rows.binary_search(&(cell.old_row, cell.new_row)).unwrap(),
                residual_columns
                    .binary_search(&(cell.old_column, cell.new_column))
                    .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let cover =
        BipartiteGraph::new(rows.len(), residual_columns.len(), &edges).minimum_vertex_cover();

    let mut selected_rows = cover
        .left
        .iter()
        .map(|&index| rows[index])
        .collect::<Vec<_>>();
    for &index in &cover.right {
        let (old, new) = residual_columns[index];
        columns.push(ChangedColumn {
            old,
            new,
            type_changed: false,
            values_changed: true,
        });
    }
    columns.sort_by_key(|column| (column.old, column.new));
    selected_rows.sort_unstable();

    let summary = SummaryChanges {
        optimal: true,
        columns,
        rows: selected_rows,
    };
    debug_assert!(changes.cells.iter().all(|cell| {
        summary
            .columns
            .iter()
            .any(|column| column.old == cell.old_column && column.new == cell.new_column)
            || summary.rows.contains(&(cell.old_row, cell.new_row))
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
    use super::{BipartiteGraph, VertexCover, summarize};
    use crate::cells::{CellChanges, ChangedCell, ChangedColumn};

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
            cells: vec![
                changed_cell(0, 0, 1, 1),
                changed_cell(1, 0, 0, 1),
                changed_cell(0, 2, 1, 0),
            ],
            columns: vec![
                changed_column(0, 1, true, true),
                changed_column(2, 0, false, true),
                changed_column(3, 3, true, false),
            ],
        };

        assert_eq!(
            summarize(&changes),
            super::SummaryChanges {
                optimal: true,
                columns: vec![
                    changed_column(0, 1, true, true),
                    changed_column(3, 3, true, false),
                ],
                rows: vec![(0, 1)],
            }
        );
    }

    #[test]
    fn selected_vertices_retain_moved_identities() {
        let column_dominant = CellChanges {
            cells: vec![changed_cell(0, 2, 2, 0), changed_cell(1, 2, 0, 0)],
            columns: vec![changed_column(2, 0, false, true)],
        };
        let row_dominant = CellChanges {
            cells: vec![changed_cell(0, 1, 2, 2), changed_cell(0, 2, 2, 1)],
            columns: vec![
                changed_column(1, 2, false, true),
                changed_column(2, 1, false, true),
            ],
        };

        assert_eq!(
            summarize(&column_dominant).columns,
            [changed_column(2, 0, false, true)]
        );
        assert_eq!(summarize(&row_dominant).rows, [(0, 2)]);
    }

    fn changed_cell(
        old_row: usize,
        old_column: usize,
        new_row: usize,
        new_column: usize,
    ) -> ChangedCell {
        ChangedCell {
            old_row,
            old_column,
            new_row,
            new_column,
        }
    }

    fn changed_column(
        old: usize,
        new: usize,
        type_changed: bool,
        values_changed: bool,
    ) -> ChangedColumn {
        ChangedColumn {
            old,
            new,
            type_changed,
            values_changed,
        }
    }
}
