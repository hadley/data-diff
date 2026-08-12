//! Reduce changed cells to a minimum-weight set of row and column edits.
//!
//! "Parsimonious" means minimum description length rather than fewest events.
//! Each candidate event is weighted by the bits needed to state it: a row edit
//! names its row among the matched rows and its changed columns among the
//! identified ones, `log2(R) + log2(C choose c)`, and a column edit
//! symmetrically `log2(C) + log2(R choose k)`, where `R` and `C` are the
//! matched-row and identified-column counts of the whole comparison and `c`
//! and `k` are the vertex's changed-cell counts. The cover that minimizes the
//! summed weight is found exactly, as a minimum s–t cut.
//!
//! The weights are irrational, so byte-identical output requires fixed
//! precision: they are computed in `f64` and rounded to centi-bits (`i64`),
//! which is far finer than any realistic gap between covers. Ties after
//! rounding break by the solver's deterministic traversal order.
//!
//! The sum is a deliberate upper bound on description length rather than an
//! exact bit count: a cell covered by both its row and its column is specified
//! twice. What `optimal` reports is therefore a minimum-weight cover under
//! these weights, not a minimum description length.

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
    /// The changed identified columns, as ascending zero-based new-side
    /// positions — the same cover-independent fact as `changes`, which is
    /// its length.
    pub columns: Vec<usize>,
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
/// `matched_rows` and `identified_columns` are the whole comparison's counts,
/// which the description-length weights are stated against; a component's
/// own counts would make the same edit cost differently depending on what
/// else changed.
///
/// `cap` bounds the residual cells the exact minimum-weight cover is computed
/// over. At or below it, the min-cut solve runs with no internal metering —
/// the cap itself bounds its worst case. Above it, the solver is skipped
/// entirely for a valid cover found in one linear pass, and `optimal` is
/// `false`: past a few thousand changed cells nobody inspects cell-level
/// minimality, and a cover never exceeds the changed columns anyway, so the
/// output was never what grew.
///
/// Every chosen event then counts the changed cells incident to it, over the
/// whole cell set rather than over the graph. A row edit and a column edit that
/// cross both count the cell they share, so the counts do not sum to the number
/// of changed cells: each is a fact about its own row or column, which keeps it
/// checkable against the data and independent of which tied minimum cover was
/// chosen.
pub(crate) fn summarize(
    changes: &CellChanges,
    forced: &[(usize, usize)],
    cap: usize,
    matched_rows: usize,
    identified_columns: usize,
) -> SummaryChanges {
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

    // The description-length weights, stated against the whole comparison.
    // Every vertex touches an edge, so both counts are positive whenever a
    // weight is computed.
    let weight = |bits: f64| (bits * 100.0).round() as i64;
    let row_weight = |degree: usize| {
        weight((matched_rows as f64).log2() + log2_choose(identified_columns, degree))
    };
    let column_weight = |degree: usize| {
        weight((identified_columns as f64).log2() + log2_choose(matched_rows, degree))
    };
    let graph = BipartiteGraph::new(rows.len(), residual_columns.len(), &edges);
    let left_weights = graph
        .adjacency
        .iter()
        .map(|neighbors| row_weight(neighbors.len()))
        .collect::<Vec<_>>();
    let mut right_degrees = vec![0; residual_columns.len()];
    for &(_, right) in &edges {
        right_degrees[right] += 1;
    }
    let right_weights = right_degrees
        .into_iter()
        .map(column_weight)
        .collect::<Vec<_>>();

    let optimal = edges.len() <= cap;
    let cover = if optimal {
        graph.minimum_vertex_cover(&left_weights, &right_weights)
    } else {
        fallback_cover(&graph, &left_weights, &right_weights)
    };

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
        .map(|row| {
            let mut columns = changes
                .columns
                .iter()
                .filter(|column| column.rows.contains(&row))
                .map(|column| column.new)
                .collect::<Vec<_>>();
            columns.sort_unstable();
            SummaryRow {
                old: row.0,
                new: row.1,
                changes: columns.len(),
                columns,
            }
        })
        .collect::<Vec<_>>();

    let summary = SummaryChanges {
        optimal,
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

/// The base-2 logarithm of `n choose k`, summed term by term.
///
/// The weights are irrational almost everywhere, which is why the callers
/// round to centi-bits: two runs must agree bit for bit, and a fixed-precision
/// sum of logarithms is reproducible where a floating-point comparison of
/// exact values would not need to be.
fn log2_choose(n: usize, k: usize) -> f64 {
    let k = k.min(n - k);
    (1..=k)
        .map(|i| ((n - k + i) as f64 / i as f64).log2())
        .sum()
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

    /// The weighted König cover, as a minimum s–t cut: source-to-row edges
    /// carry the row weights, column-to-sink edges the column weights, and
    /// cell edges an infinite capacity, so a finite cut picks exactly a vertex
    /// cover and the minimum cut picks the lightest one. The cover is
    /// recovered from the reachable set after the final breadth-first search:
    /// selected rows are the unreachable left vertices, selected columns the
    /// reachable right ones. That set is the inclusion-minimal source side of
    /// a minimum cut, which is unique, so the cover does not depend on which
    /// maximum flow the solver happened to find.
    fn minimum_vertex_cover(&self, left_weights: &[i64], right_weights: &[i64]) -> VertexCover {
        let source = self.left_count + self.right_count;
        let sink = source + 1;
        let mut network = FlowNetwork::new(sink + 1);
        let infinite = left_weights.iter().chain(right_weights.iter()).sum::<i64>() + 1;
        for (left, &weight) in left_weights.iter().enumerate() {
            network.add_edge(source, left, weight);
        }
        for (left, rights) in self.adjacency.iter().enumerate() {
            for &right in rights {
                network.add_edge(left, self.left_count + right, infinite);
            }
        }
        for (right, &weight) in right_weights.iter().enumerate() {
            network.add_edge(self.left_count + right, sink, weight);
        }

        network.maximum_flow(source, sink);
        let reachable = network.reachable_from(source);
        let cover = VertexCover {
            left: (0..self.left_count)
                .filter(|&left| !reachable[left])
                .collect(),
            right: (0..self.right_count)
                .filter(|&right| reachable[self.left_count + right])
                .collect(),
        };
        debug_assert!(cover.covers(self));
        debug_assert_eq!(
            cover.weight(left_weights, right_weights),
            network.maximum_flow_value
        );
        cover
    }
}

/// Dinic's maximum flow, deterministic by construction: adjacency lists are
/// built in ascending vertex order and both searches consume them in that
/// order.
struct FlowNetwork {
    edges: Vec<FlowEdge>,
    adjacency: Vec<Vec<usize>>,
    maximum_flow_value: i64,
}

struct FlowEdge {
    to: usize,
    capacity: i64,
    reverse: usize,
}

impl FlowNetwork {
    fn new(vertices: usize) -> Self {
        Self {
            edges: Vec::new(),
            adjacency: vec![Vec::new(); vertices],
            maximum_flow_value: 0,
        }
    }

    fn add_edge(&mut self, from: usize, to: usize, capacity: i64) {
        let forward = self.edges.len();
        self.edges.push(FlowEdge {
            to,
            capacity,
            reverse: forward + 1,
        });
        self.edges.push(FlowEdge {
            to: from,
            capacity: 0,
            reverse: forward,
        });
        self.adjacency[from].push(forward);
        self.adjacency[to].push(forward + 1);
    }

    fn maximum_flow(&mut self, source: usize, sink: usize) {
        let mut levels = vec![0; self.adjacency.len()];
        let mut next = vec![0; self.adjacency.len()];
        while self.layer_graph(source, sink, &mut levels) {
            next.fill(0);
            loop {
                let pushed = self.push(source, sink, i64::MAX, &levels, &mut next);
                if pushed == 0 {
                    break;
                }
                self.maximum_flow_value += pushed;
            }
        }
    }

    /// Breadth-first search over residual edges, layering the graph until the
    /// sink is reached; false when it is cut off, which ends the solve.
    fn layer_graph(&self, source: usize, sink: usize, levels: &mut [usize]) -> bool {
        levels.fill(usize::MAX);
        levels[source] = 0;
        let mut queue = VecDeque::from([source]);
        while let Some(vertex) = queue.pop_front() {
            for &edge in &self.adjacency[vertex] {
                let edge = &self.edges[edge];
                if edge.capacity > 0 && levels[edge.to] == usize::MAX {
                    levels[edge.to] = levels[vertex] + 1;
                    queue.push_back(edge.to);
                }
            }
        }
        levels[sink] != usize::MAX
    }

    /// Depth-first search along the layered graph, returning the flow one
    /// augmentation carried — which can be less than offered, the path
    /// narrowing downstream — or zero when the layer admits no further flow.
    fn push(
        &mut self,
        vertex: usize,
        sink: usize,
        flow: i64,
        levels: &[usize],
        next: &mut [usize],
    ) -> i64 {
        if vertex == sink {
            return flow;
        }
        while next[vertex] < self.adjacency[vertex].len() {
            let index = self.adjacency[vertex][next[vertex]];
            let edge = &self.edges[index];
            if edge.capacity > 0 && levels[edge.to] == levels[vertex] + 1 {
                let pushed = self.push(edge.to, sink, flow.min(edge.capacity), levels, next);
                if pushed > 0 {
                    self.edges[index].capacity -= pushed;
                    let reverse = self.edges[index].reverse;
                    self.edges[reverse].capacity += pushed;
                    return pushed;
                }
            }
            next[vertex] += 1;
        }
        0
    }

    /// The vertices reachable from `source` over residual edges — the source
    /// side of the minimum cut once the flow is maximum.
    fn reachable_from(&self, source: usize) -> Vec<bool> {
        let mut reachable = vec![false; self.adjacency.len()];
        reachable[source] = true;
        let mut queue = VecDeque::from([source]);
        while let Some(vertex) = queue.pop_front() {
            for &edge in &self.adjacency[vertex] {
                let edge = &self.edges[edge];
                if edge.capacity > 0 && !reachable[edge.to] {
                    reachable[edge.to] = true;
                    queue.push_back(edge.to);
                }
            }
        }
        reachable
    }
}

/// A valid cover found without solving, for a graph past the cell cap.
///
/// Each connected component contributes its lighter side by summed weight —
/// its columns when the tie is even, keeping the column-flavored description
/// where either would do. Every edge has both its endpoints in some component,
/// so choosing a whole side of each covers every cell; the weight is not
/// minimal, which is exactly what `optimal: false` reports. One linear pass,
/// traversal in ascending vertex order, so the cover is deterministic.
fn fallback_cover(
    graph: &BipartiteGraph,
    left_weights: &[i64],
    right_weights: &[i64],
) -> VertexCover {
    let mut left_adjacency = vec![Vec::new(); graph.left_count];
    let mut right_adjacency = vec![Vec::new(); graph.right_count];
    for (left, rights) in graph.adjacency.iter().enumerate() {
        for &right in rights {
            left_adjacency[left].push(right);
            right_adjacency[right].push(left);
        }
    }

    let mut left_seen = vec![false; graph.left_count];
    let mut right_seen = vec![false; graph.right_count];
    let mut cover = VertexCover::default();
    for start in 0..graph.left_count {
        if left_seen[start] || left_adjacency[start].is_empty() {
            continue;
        }
        // Collect one component. Isolated vertices touch no edge and need no
        // cover, and every component holds at least one left vertex, an edge
        // having a left endpoint by construction.
        let mut lefts = Vec::new();
        let mut rights = Vec::new();
        let mut queue = VecDeque::from([start]);
        left_seen[start] = true;
        while let Some(left) = queue.pop_front() {
            lefts.push(left);
            for &right in &left_adjacency[left] {
                if right_seen[right] {
                    continue;
                }
                right_seen[right] = true;
                rights.push(right);
                for &next in &right_adjacency[right] {
                    if !left_seen[next] {
                        left_seen[next] = true;
                        queue.push_back(next);
                    }
                }
            }
        }
        let left_weight = lefts.iter().map(|&left| left_weights[left]).sum::<i64>();
        let right_weight = rights
            .iter()
            .map(|&right| right_weights[right])
            .sum::<i64>();
        if left_weight < right_weight {
            cover.left.extend(lefts);
        } else {
            cover.right.extend(rights);
        }
    }
    cover
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct VertexCover {
    left: Vec<usize>,
    right: Vec<usize>,
}

impl VertexCover {
    fn weight(&self, left_weights: &[i64], right_weights: &[i64]) -> i64 {
        self.left
            .iter()
            .map(|&left| left_weights[left])
            .sum::<i64>()
            + self
                .right
                .iter()
                .map(|&right| right_weights[right])
                .sum::<i64>()
    }

    fn covers(&self, graph: &BipartiteGraph) -> bool {
        graph.adjacency.iter().enumerate().all(|(left, rights)| {
            rights
                .iter()
                .all(|right| self.left.contains(&left) || self.right.contains(right))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{BipartiteGraph, SummaryChanges, SummaryColumn, SummaryRow, VertexCover};
    use crate::cells::{CellChanges, ColumnChanges};

    /// The whole-comparison counts the weights are stated against, chosen
    /// large enough that the description-length trade-offs show.
    const ROWS: usize = 9_980;
    const COLUMNS: usize = 10;

    fn summarize(changes: &CellChanges) -> SummaryChanges {
        super::summarize(changes, &[], usize::MAX, ROWS, COLUMNS)
    }

    fn graph(left: usize, right: usize, edges: &[(usize, usize)]) -> BipartiteGraph {
        BipartiteGraph::new(left, right, edges)
    }

    /// Vertex weights for a graph taken to be the whole comparison: `R` and
    /// `C` are its own side counts.
    fn weights(graph: &BipartiteGraph) -> (Vec<i64>, Vec<i64>) {
        let weight = |bits: f64| (bits * 100.0).round() as i64;
        let mut right_degrees = vec![0; graph.right_count];
        for rights in &graph.adjacency {
            for &right in rights {
                right_degrees[right] += 1;
            }
        }
        let left = graph
            .adjacency
            .iter()
            .map(|rights| {
                weight(
                    (graph.left_count as f64).log2()
                        + super::log2_choose(graph.right_count, rights.len()),
                )
            })
            .collect();
        let right = right_degrees
            .into_iter()
            .map(|degree| {
                weight(
                    (graph.right_count as f64).log2()
                        + super::log2_choose(graph.left_count, degree),
                )
            })
            .collect();
        (left, right)
    }

    fn weighted_cover(graph: &BipartiteGraph) -> VertexCover {
        let (left, right) = weights(graph);
        graph.minimum_vertex_cover(&left, &right)
    }

    fn is_cover(graph: &BipartiteGraph, cover: &VertexCover) -> bool {
        cover.covers(graph)
    }

    fn brute_force_optimum(graph: &BipartiteGraph) -> i64 {
        let (left_weights, right_weights) = weights(graph);
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
                is_cover(graph, &cover).then(|| cover.weight(&left_weights, &right_weights))
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
    fn the_weight_function_is_exact_at_its_edges() {
        // k = 0 and k = n both leave a single possibility, costing no bits.
        assert_eq!(super::log2_choose(10, 0), 0.0);
        assert_eq!(super::log2_choose(10, 10), 0.0);
        assert_eq!(super::log2_choose(10, 1), 10_f64.log2());
        // The symmetry the minimization relies on.
        assert_eq!(super::log2_choose(10, 3), super::log2_choose(10, 7));
    }

    #[test]
    fn brute_force_oracle_finds_row_and_column_optima() {
        // A row changed in three columns costs log2(3) + log2(1) = ... less
        // than three columns each changed in one row, and symmetrically.
        let row_star = graph(1, 3, &[(0, 0), (0, 1), (0, 2)]);
        let column_star = graph(3, 1, &[(0, 0), (1, 0), (2, 0)]);
        let (left, right) = weights(&row_star);
        assert_eq!(
            weighted_cover(&row_star).weight(&left, &right),
            brute_force_optimum(&row_star)
        );
        let (left, right) = weights(&column_star);
        assert_eq!(
            weighted_cover(&column_star).weight(&left, &right),
            brute_force_optimum(&column_star)
        );
    }

    #[test]
    fn exact_cover_handles_representative_shapes() {
        let cases = [
            (graph(0, 0, &[]), VertexCover::default()),
            (
                // Both weights are zero; the minimal source side of the cut
                // leaves the row unreachable, so the row is selected.
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
            assert_eq!(weighted_cover(&graph), expected);
        }
    }

    #[test]
    fn tied_cover_is_deterministic_and_ignores_isolates() {
        // A square of equal weights: every vertex costs one bit, and the
        // minimum cut's unique minimal source side selects the rows.
        let graph = graph(3, 3, &[(0, 0), (0, 1), (1, 0), (1, 1)]);
        let first = weighted_cover(&graph);

        assert_eq!(
            first,
            VertexCover {
                left: vec![0, 1],
                right: vec![],
            }
        );
        assert_eq!(weighted_cover(&graph), first);
    }

    #[test]
    fn disconnected_components_choose_a_row_and_a_column() {
        let graph = graph(3, 3, &[(0, 0), (0, 1), (1, 2), (2, 2)]);

        assert_eq!(
            weighted_cover(&graph),
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
                    let (left_weights, right_weights) = weights(&graph);
                    let cover = graph.minimum_vertex_cover(&left_weights, &right_weights);

                    assert!(is_cover(&graph, &cover));
                    assert_eq!(
                        cover.weight(&left_weights, &right_weights),
                        brute_force_optimum(&graph),
                        "edges {edges:?}"
                    );
                    assert_eq!(
                        graph.minimum_vertex_cover(&left_weights, &right_weights),
                        cover
                    );
                }
            }
        }
    }

    #[test]
    fn a_sparse_systematic_column_stays_a_column_edit() {
        // The issue's first case: `price` changes in every twentieth row of a
        // 9,980-row, ten-column comparison. Stating the column costs the
        // column's name and its 499 changed rows (~47 bits); stating the 499
        // rows costs each row's position and changed column (~28 bits apiece).
        let changes = CellChanges {
            columns: vec![changed_column(
                1,
                1,
                false,
                &(0..499)
                    .map(|index| (index * 20, index * 20))
                    .collect::<Vec<_>>(),
            )],
            ..CellChanges::default()
        };

        let summary = summarize(&changes);

        assert_eq!(summary.columns, [summary_column(1, 1, false, 499)]);
        assert!(summary.rows.is_empty());
    }

    #[test]
    fn a_dense_rectangle_flips_to_row_edits() {
        // The issue's second case: a 50-by-5 rectangle. Fifty rows at ~21
        // bits apiece underbid five columns each naming 50 of 9,980 rows.
        let rows = (0..50).map(|row| (row, row)).collect::<Vec<_>>();
        let changes = CellChanges {
            columns: (0..5)
                .map(|column| changed_column(column, column, false, &rows))
                .collect(),
            ..CellChanges::default()
        };

        let summary = summarize(&changes);

        assert!(summary.columns.is_empty());
        assert_eq!(summary.rows.len(), 50);
        assert!(summary.rows.iter().all(|row| row.changes == 5));
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
                rows: vec![summary_row(0, 1, &[0, 1])],
            }
        );
    }

    #[test]
    fn a_forced_column_leaves_the_optimizer_and_takes_its_cells_with_it() {
        // One column changing in both rows, and each row changing in a column
        // of its own. Covering the two rows underbids covering the three
        // columns, so the answer is two row edits.
        let changes = CellChanges {
            columns: vec![
                changed_column(1, 1, false, &[(0, 0), (1, 1)]),
                changed_column(2, 2, false, &[(0, 0)]),
                changed_column(3, 3, false, &[(1, 1)]),
            ],
            ..CellChanges::default()
        };

        let free = summarize(&changes);
        assert_eq!(
            free.rows,
            [summary_row(0, 0, &[1, 2]), summary_row(1, 1, &[1, 3])]
        );
        assert!(free.columns.is_empty());

        // Hint the two single-cell columns and they leave the graph, taking
        // their cells with them. What is left to cover is one column spanning
        // both rows, so the minimum is now that column and there is no row edit
        // at all. The row summary changed because the graph did, not because
        // anything overrode the answer it produced.
        let forced = super::summarize(&changes, &[(2, 2), (3, 3)], usize::MAX, ROWS, COLUMNS);

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

        // Weighed against the whole comparison the three rows would underbid
        // the crossing pair, so this fixture states its own small counts to
        // keep the crossing the minimum.
        let summary = super::summarize(&changes, &[], usize::MAX, 3, 3);

        // Three and three over five cells. The counts are deliberately not a
        // partition: each is a fact about its own row or column, which is what
        // makes it checkable against the data and keeps it independent of which
        // minimum cover was chosen.
        assert_eq!(summary.columns, [summary_column(2, 2, false, 3)]);
        assert_eq!(summary.rows, [summary_row(0, 0, &[0, 1, 2])]);
    }

    #[test]
    fn a_row_counts_cells_in_a_column_held_out_of_the_graph() {
        // "a" changes in both rows and "b" in the first, so covering the rows
        // is the lighter description until "a" is hinted out of the graph.
        let changes = CellChanges {
            columns: vec![
                changed_column(1, 1, false, &[(0, 0), (1, 1)]),
                changed_column(2, 2, false, &[(0, 0)]),
            ],
            ..CellChanges::default()
        };

        let forced = super::summarize(&changes, &[(1, 1)], usize::MAX, ROWS, COLUMNS);

        // Row 0 is reported for the one cell left to cover, and counts two:
        // the cell in the hinted column is still a changed cell in that row. A
        // hint moves which events are reported, not what is true of a row.
        assert_eq!(forced.columns, [summary_column(1, 1, false, 2)]);
        assert_eq!(forced.rows, [summary_row(0, 0, &[1, 2])]);
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
        assert_eq!(summarize(&row_dominant).rows, [summary_row(0, 2, &[1, 2])]);
    }

    #[test]
    fn a_diff_at_the_cap_solves_exactly_and_one_above_it_falls_back() {
        let changes = CellChanges {
            columns: vec![
                changed_column(1, 1, false, &[(0, 0), (1, 1)]),
                changed_column(2, 2, false, &[(0, 0), (1, 1)]),
            ],
            ..CellChanges::default()
        };

        // Four changed cells at a cap of four solve exactly: the tied minimum
        // picks the two rows, as the solver's own determinism test pins.
        let exact = super::summarize(&changes, &[], 4, 2, 2);
        assert!(exact.optimal);
        assert_eq!(
            exact.rows,
            [summary_row(0, 0, &[1, 2]), summary_row(1, 1, &[1, 2])]
        );
        assert!(exact.columns.is_empty());

        // One cell over the cap skips the solver. The component's two sides
        // tie on weight, and a tie keeps the column-flavored description;
        // every cell is still covered, and `optimal` says the description was
        // not minimized rather than that anything is missing.
        let capped = super::summarize(&changes, &[], 3, 2, 2);
        assert!(!capped.optimal);
        assert!(capped.rows.is_empty());
        assert_eq!(
            capped.columns,
            [
                summary_column(1, 1, false, 2),
                summary_column(2, 2, false, 2)
            ]
        );
    }

    #[test]
    fn the_fallback_covers_each_component_by_its_lighter_side() {
        // Two disconnected shapes: a column changed in three rows, and one row
        // changed in three columns of its own.
        let changes = CellChanges {
            columns: vec![
                changed_column(1, 1, false, &[(0, 0), (1, 1), (2, 2)]),
                changed_column(2, 2, false, &[(5, 5)]),
                changed_column(3, 3, false, &[(5, 5)]),
                changed_column(4, 4, false, &[(5, 5)]),
            ],
            ..CellChanges::default()
        };

        let capped = super::summarize(&changes, &[], 5, ROWS, COLUMNS);

        // The first component's lighter side is its one column, the second's
        // its one row, and each event still counts every cell incident to it.
        assert!(!capped.optimal);
        assert_eq!(capped.columns, [summary_column(1, 1, false, 3)]);
        assert_eq!(capped.rows, [summary_row(5, 5, &[2, 3, 4])]);
    }

    #[test]
    fn the_fallback_compares_summed_weights_and_keeps_columns_on_a_tie() {
        // One row changed in two columns against those two columns: the count
        // prefers the row either way, but the weights are what is compared —
        // one row's ~33 bits against two columns' ~43.
        let row_light = CellChanges {
            columns: vec![
                changed_column(0, 0, false, &[(0, 0)]),
                changed_column(1, 1, false, &[(0, 0)]),
            ],
            ..CellChanges::default()
        };
        let capped = super::summarize(&row_light, &[], 1, ROWS, COLUMNS);
        assert_eq!(capped.rows, [summary_row(0, 0, &[0, 1])]);
        assert!(capped.columns.is_empty());

        // A single edge leaves the two sides at exactly equal weight —
        // log2(R) + log2(C) either way — and the tie keeps the column.
        let one_edge = CellChanges {
            columns: vec![changed_column(0, 0, false, &[(0, 0)])],
            ..CellChanges::default()
        };
        let tied = super::summarize(&one_edge, &[], 0, ROWS, ROWS);
        assert!(tied.rows.is_empty());
        assert_eq!(tied.columns, [summary_column(0, 0, false, 1)]);
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

    fn summary_row(old: usize, new: usize, columns: &[usize]) -> SummaryRow {
        SummaryRow {
            old,
            new,
            changes: columns.len(),
            columns: columns.to_vec(),
        }
    }
}
