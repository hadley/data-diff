# data-diff-ui

The interactive UI for data-diff: a sidebar listing every component of the diff — schema, columns, rows edited (per changed-column group), added, dropped, moved, fanout, cells — with the selected component's table filling the main panel, scrolling in both directions with keys and column names pinned, and loading rows lazily from the input tables as you scroll. A small dependency-free Rust server (`ui/server`) holds the session and serves the Preact frontend (`ui/src`, built to `ui/dist`) straight to the browser.

## Demo data

A 10,000-row inventory pair with a bit of everything — one edited column (`price`, ~500 scattered edits), a 50×5 rectangle of edits in `r1`–`r5`, an added column (`status`), a dropped column (`legacy`), 50 added rows, and 20 dropped rows:

```
cargo run --example generate_ui_demo
cargo run -p data-diff-ui -- ui/demo-old.parquet ui/demo-new.parquet --key id
```

Generation is deterministic (fixed-seed), so the pair is byte-identical on every run. The files are gitignored.

## Running

Build the frontend once, then run the server:

```
npm install && npm run build
cargo run -p data-diff-ui -- old.parquet new.parquet --key id
```

The server prints its address (default http://127.0.0.1:9471, `--port` to change) and opens it in your browser. Launched without paths, the app opens on a form asking for them. The server finds the built frontend at `ui/dist` under the working directory or next to a `target/`-built executable; set `DATA_DIFF_UI_DIST` to point elsewhere.

For frontend development, just run the server in a debug build — it starts `npm run build:watch` itself and reloads the browser whenever the rebuilt `ui/dist` changes:

```
cargo run -p data-diff-ui -- old.parquet new.parquet
```

## Launching with paths

The positional arguments carry the same `--key`, `--hint`, and `--hints` options as the CLI:

```
data-diff-ui old.parquet new.parquet --key id
data-diff-ui old.parquet new.parquet --key id --hint "col_rename(name -> label)"
```

## Use as a git difftool

Git passes the two versions of a file as `$LOCAL $REMOTE`, which is exactly the positional argument order, so the app wires in as a difftool:

```
git config --global diff.tool data-diff-ui
git config --global difftool.data-diff-ui.cmd 'data-diff-ui "$LOCAL" "$REMOTE"'
```

Mark Parquet files as binary-diffable in `.gitattributes` so git offers the tool for them:

```
*.parquet binary
```

Then `git difftool` on a changed Parquet file starts the server and opens the diff in your browser. (This suits repositories that track small Parquet fixtures directly, not LFS pointer files.)
