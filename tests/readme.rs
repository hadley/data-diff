//! Checks that every command transcript in `demo/README.md` still prints what
//! the file says it prints.
//!
//! A `console` block whose first line is `$ data-diff ...` is a transcript: the
//! command, then the output it produced. The test runs the command again from
//! the repository root and compares. Setting `UPDATE_README=1` rewrites the
//! transcripts in place instead of failing, so refreshing the file after a
//! deliberate change to the format is one command rather than an editing pass:
//!
//! ```console
//! UPDATE_README=1 cargo test --test readme
//! ```
//!
//! Only the output is rewritten; how the command itself is written is left
//! alone. Blocks that run something other than `data-diff` are ignored.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One `$ data-diff ...` transcript found in the file.
struct Transcript {
    /// The command, joined across any continuation lines.
    command: String,
    /// The output the file claims it prints, without a trailing newline.
    expected: String,
    /// Byte range of that output, for rewriting it in place.
    output: Range<usize>,
}

#[test]
fn demo_readme_shows_what_the_commands_print() {
    let path = readme_path();
    let text = std::fs::read_to_string(&path).expect("demo/README.md is readable");
    let transcripts = parse(&text);
    assert!(
        !transcripts.is_empty(),
        "demo/README.md has no `$ data-diff` transcripts to check"
    );

    let updating = std::env::var_os("UPDATE_README").is_some_and(|value| value != "0");
    let mut stale = Vec::new();
    let mut rewritten = text.clone();
    // Splicing back to front keeps the ranges before each edit valid.
    for transcript in transcripts.iter().rev() {
        let actual = run(&transcript.command);
        if actual == transcript.expected {
            continue;
        }
        if updating {
            let replacement = if actual.is_empty() {
                String::new()
            } else {
                format!("{actual}\n")
            };
            rewritten.replace_range(transcript.output.clone(), &replacement);
        } else {
            stale.push(format!(
                "$ {}\n--- demo/README.md says ---\n{}\n--- it prints ---\n{}",
                transcript.command, transcript.expected, actual
            ));
        }
    }

    if updating {
        if rewritten != text {
            std::fs::write(&path, rewritten).expect("demo/README.md is writable");
        }
        return;
    }

    assert!(
        stale.is_empty(),
        "{} of {} transcripts in demo/README.md are out of date. \
         Re-run with UPDATE_README=1 to refresh them.\n\n{}",
        stale.len(),
        transcripts.len(),
        stale.join("\n\n")
    );
}

#[test]
fn every_demo_command_is_a_checked_transcript() {
    let text = std::fs::read_to_string(readme_path()).expect("demo/README.md is readable");
    let unchecked: Vec<_> = blocks(&text)
        .map(|(_, body)| body.lines().next().unwrap_or_default())
        .filter(|first| first.starts_with("data-diff "))
        .collect();

    // A command shown without its output is a claim nothing checks, which is
    // the drift this file exists to prevent.
    assert!(
        unchecked.is_empty(),
        "demo/README.md shows commands with no output:\n{}",
        unchecked.join("\n")
    );
}

#[test]
fn every_demo_fixture_is_used() {
    let text = std::fs::read_to_string(readme_path()).expect("demo/README.md is readable");
    let mut orphans: Vec<_> = std::fs::read_dir(repo_root().join("demo"))
        .expect("demo/ is readable")
        .map(|entry| entry.expect("demo/ entry is readable").file_name())
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.ends_with(".parquet"))
        .filter(|name| !text.contains(&format!("demo/{name}")))
        .collect();
    orphans.sort();

    // A fixture no command reads is dead weight, and the example that wrote it
    // is too. Deleting a section should take both with it.
    assert!(
        orphans.is_empty(),
        "demo/ holds fixtures no command in demo/README.md reads:\n{}\n\
         Delete them and the examples/generate_demo.rs code that writes them.",
        orphans.join("\n")
    );
}

fn readme_path() -> PathBuf {
    repo_root().join("demo/README.md")
}

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Every fenced `console` block, as the byte range of its body and the body.
fn blocks(text: &str) -> impl Iterator<Item = (Range<usize>, &str)> {
    const OPEN: &str = "```console\n";
    const CLOSE: &str = "```";
    let mut rest = 0;
    std::iter::from_fn(move || {
        let open = text[rest..].find(OPEN)? + rest;
        let start = open + OPEN.len();
        let end = start + text[start..].find(CLOSE)?;
        rest = end + CLOSE.len();
        Some((start..end, &text[start..end]))
    })
}

/// The `$ data-diff ...` transcripts, in the order the file gives them.
fn parse(text: &str) -> Vec<Transcript> {
    blocks(text)
        .filter_map(|(range, body)| {
            let first = body.lines().next()?;
            let mut command = first.strip_prefix("$ ")?.to_string();
            if !command.starts_with("data-diff ") {
                return None;
            }
            // A long command is wrapped across lines with a trailing backslash.
            let mut consumed = first.len();
            while let Some(head) = command.strip_suffix('\\') {
                command = head.trim_end().to_string();
                let next = body[consumed + 1..]
                    .lines()
                    .next()
                    .expect("a continued command has a next line");
                command.push(' ');
                command.push_str(next.trim());
                consumed += 1 + next.len();
            }
            // The output is whatever follows the newline ending the command.
            let output = range.start + (consumed + 1).min(body.len())..range.end;
            Some(Transcript {
                command,
                expected: text[output.clone()].trim_end().to_string(),
                output,
            })
        })
        .collect()
}

/// Run one command from the repository root and return what a terminal would
/// have shown: standard output, then anything on standard error.
fn run(command: &str) -> String {
    let mut args = split(command).into_iter();
    let program = args.next().expect("a command names a program");
    assert_eq!(
        program, "data-diff",
        "only data-diff transcripts are checked"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_data-diff"))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("failed to run data-diff");

    let mut shown = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    shown.push_str(&String::from_utf8(output.stderr).expect("stderr is UTF-8"));
    shown.trim_end().to_string()
}

/// Split a command the way a shell would, honouring single and double quotes.
/// Hints arrive quoted, and nothing in this file needs more than that.
fn split(command: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote = None;
    for character in command.chars() {
        match (quote, character) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), c) => current.push(c),
            (None, c @ ('\'' | '"')) => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, c) => {
                current.push(c);
                started = true;
            }
        }
    }
    assert!(quote.is_none(), "unbalanced quote in: {command}");
    if started {
        args.push(current);
    }
    args
}

#[test]
fn splits_a_command_on_whitespace() {
    assert_eq!(
        split("data-diff a.parquet b.parquet --key id"),
        ["data-diff", "a.parquet", "b.parquet", "--key", "id"]
    );
}

#[test]
fn keeps_a_quoted_argument_whole() {
    assert_eq!(
        split("data-diff --hint 'col_rename(a -> b)'"),
        ["data-diff", "--hint", "col_rename(a -> b)"]
    );
}

#[test]
fn keeps_an_empty_quoted_argument() {
    assert_eq!(split("data-diff --key ''"), ["data-diff", "--key", ""]);
}

#[test]
fn reads_a_transcript_and_locates_its_output() {
    let text =
        "before\n```console\n$ data-diff a.parquet b.parquet --key id\ntable_key([id])\n```\n";
    let parsed = parse(text);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].command, "data-diff a.parquet b.parquet --key id");
    assert_eq!(parsed[0].expected, "table_key([id])");
    assert_eq!(&text[parsed[0].output.clone()], "table_key([id])\n");
}

#[test]
fn joins_a_command_continued_across_lines() {
    let text = "```console\n$ data-diff a.parquet b.parquet \\\n    --key id\ntable_key([id])\n```";
    let parsed = parse(text);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].command, "data-diff a.parquet b.parquet --key id");
    assert_eq!(parsed[0].expected, "table_key([id])");
}

#[test]
fn reads_a_transcript_that_printed_nothing() {
    let text = "```console\n$ data-diff a.parquet b.parquet\n```";
    let parsed = parse(text);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].expected, "");
    assert!(parsed[0].output.is_empty());
}

#[test]
fn ignores_a_block_that_runs_something_else() {
    assert!(parse("```console\n$ cargo run --example generate_demo\n```").is_empty());
    assert!(parse("```console\ncargo install --path .\n```").is_empty());
}
