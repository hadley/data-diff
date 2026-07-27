---
name: create-pr
description: >
  Create a GitHub pull request for the current work. Use when the user asks to
  open, create, or draft a PR (e.g. "make a PR", "open a pull request", "PR
  this"). Drafts the title and body following the style advice below, opens the
  draft locally for the user to preview, and submits with `gh pr create` only
  after they give the go-ahead.
---

# Create a pull request

Follow this whenever the user asks to make/open/draft a PR.

## PR style

- **Title:** under 70 characters.
- **Body:** the only required part is a short summary of the PR's purpose — what it does and why, understandable without reading the diff — followed by issue references if relevant (e.g. `Fixes #45`). Everything else is optional.

### Optional sections

Include the following sections only when they tells the reader something the summary doesn't. For a small, self-explanatory PR the summary alone is enough; don't pad it with sections that just restate the summary. Omit any section entirely rather than leaving it empty or boilerplate.

- `## Example` — a concrete example of the new behavior (code snippet, before/after output, short usage walkthrough; for UI changes, a screenshot — ask the user if you can't produce one). Add when seeing the behavior clarifies more than describing it does.
- `## Not included` — when the PR deliberately leaves related work out (e.g. "TUI support comes in a separate PR"), to head off "did you forget X?" review questions.
- `## Breaking changes` — when the PR changes behavior, config, or disk formats in a way that affects existing users or other packages; say what breaks and what to do about it.

Write in accessible, plain language — avoid jargon and deep implementation detail where possible. A reader who doesn't work in this codebase daily should be able to understand what the PR does and why. Save technical specifics for the optional sections, and keep them brief.

**Do not wrap long lines in the PR body** — GitHub renders every newline literally; write each paragraph as one unbroken line.

## Steps

1. **Get on a branch.** If the current branch is the default branch (`main` /
   `master`), create and switch to a new descriptive branch first — never open a
   PR from the default branch. Otherwise stay on the current branch.

2. **Commit the work.** Make sure the changes are committed. Only commit what
   belongs in this PR; don't sweep in unrelated changes. Confirm with
   `git status` and `git diff` that the working tree reflects what you intend to
   ship.

3. **Push.** Push the branch and set upstream:
   `git push -u origin <branch>`.

4. **Draft the title and body** following the **PR style** section above. Base
   the summary on the actual diff versus the base branch
   (`git diff <base>...HEAD`), not just the latest commit.

5. **Open the draft locally for the user to preview.** Write the body to a file
   and open it so the user can read it in their editor. When running inside VS
   Code, use the environment's native file-opening tool. Otherwise, use the
   Positron CLI with the file's absolute path:

   ```bash
   positron /path/to/pr-body.md
   ```

   State the **title** in chat alongside it (the file holds only the body). Then
   **stop and wait** for the user's explicit go-ahead. Do **not** create the PR
   yet. If they ask for changes, edit the file, and they'll see the update.

6. **Submit once the user approves.** Only after an explicit go-ahead, create the
   PR directly:

   ```bash
   gh pr create --base <base> --title "<title>" --body-file /path/to/pr-body.md
   ```

   Then report back the PR URL that `gh` prints.
