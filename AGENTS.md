# Agent instructions for data-diff

## Design document

* `design.md` — the durable design; all work must preserve its central invariants: deterministic reconciliation, no inferred event without underlying evidence, and continued access to the complete cell-level diff.

## Repository layout

* `src/` — the `data-diff` CLI and comparison library.
* `ui/` — `data-diff-ui`, an interactive browser UI: a Preact frontend (`ui/src`, built to `ui/dist` with Vite/npm) served by a small dependency-free Rust server (`ui/server`, cargo package `data-diff-ui`). Run with `cargo run -p data-diff-ui -- old.parquet new.parquet --key id` after `npm install && npm run build` in `ui/`. Demo data comes from `cargo run --example generate_ui_demo`. See `ui/README.md`.

## Execution rules

Development proceeds at a slow, review-first pace:

* Each large piece of work happens on its dedicated branch from `main`; never develop directly on `main`. Multiple efforts may be in flight on separate branches at once. The user asking for a plan is a strong sign that you should use a branch.
* Each branch is one separate PR-sized change.
* Present the finished branch for careful review with its changes left uncommitted; the owner alone decides when to commit.
* Every change gets isolated fixtures, integration coverage, and determinism checks; repeated runs must produce byte-identical output.
* Before presenting work, run the full test suite, strict Clippy, formatting, and diff checks.

## Settled conventions

* Unit tests stay inline in their production module as `#[cfg(test)] mod tests` blocks, the dominant Rust convention. Extracting them into separate files was considered and rejected (2026-07-25); do not re-propose it.
* Markdown prose must not use hard line breaks. Keep each paragraph and list item on one source line and let the renderer wrap it.
* `demo/README.md` documents every command with the output it produces, and `tests/readme.rs` re-runs them all, so a change to the output format will fail that test. Refresh the transcripts with `UPDATE_README=1 cargo test --test readme` rather than editing them by hand, then read the diff: prose that describes output the change has altered needs updating too, and only a person can see that. Two further tests guard the file: never add a command to it without its output, and never leave a fixture behind that no command reads — deleting a section means deleting its `demo/*.parquet` pair and the `examples/generate_demo.rs` code that writes them.
