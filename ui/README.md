# data-diff --ui

The interactive UI for data-diff: a sidebar listing every component of the diff — schema, columns, rows edited (per changed-column group), added, dropped, moved, fanout, cells — with the selected component's table filling the main panel, scrolling in both directions with keys and column names pinned, and loading rows lazily from the input tables as you scroll. A small dependency-free HTTP server (`src/ui`, behind the default `ui` cargo feature) holds the session and serves the Preact frontend (`ui/src`, built to `ui/dist`) straight to the browser. Everything is one binary: `data-diff` prints the text diff, and `data-diff --ui` serves the app.

## Demo data

A 10,000-row inventory pair with a bit of everything — one edited column (`price`, ~500 scattered edits), a 50×5 rectangle of edits in `r1`–`r5`, an added column (`status`), a dropped column (`legacy`), 50 added rows, and 20 dropped rows:

```console
cargo run --example generate_ui_demo
data-dict ui/demo-old.parquet ui/demo-new.parquet --key id --ui
```

## Frontend development

Run the server from a debug build — it starts `npm run build:watch` itself and reloads the browser whenever the rebuilt `ui/dist` changes:

```console
cargo run -- old.parquet new.parquet --key id --ui
```

## Use as a git difftool

Git passes the two versions of a file as `$LOCAL $REMOTE`, which is exactly the positional argument order, so the app wires in as a difftool:

```console
git config --global diff.tool data-diff
git config --global difftool.data-diff.cmd 'data-diff --ui "$LOCAL" "$REMOTE"'
```

Mark Parquet files as binary-diffable in `.gitattributes` so git offers the tool for them:

```
*.parquet binary
```

Then `git difftool` on a changed Parquet file starts the server and opens the diff in your browser. (This suits repositories that track small Parquet fixtures directly, not LFS pointer files.)
