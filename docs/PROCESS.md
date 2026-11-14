[English](PROCESS.md) | [中文](PROCESS.zh.md)

# Delivery process

How a change moves from the shared task board to `main` and into a release. This
document is normative for everyone working in the repository; contribution rules
live in [CONTRIBUTING.md](../CONTRIBUTING.md) and language rules in
[LANGUAGE.md](LANGUAGE.md).

## 1. Roles

| Role | Owns | Decides | Reports to |
|---|---|---|---|
| Lead | scope, task board, review gate, release approval | what ships and when | none |
| Backend | `crates/` - daemon, CLI, protocol | module layout, error model | Lead |
| Extension | `extension/` - MV3 package | extension internals | Lead |
| QA | verification, acceptance runs, regression | pass or fail against the acceptance criteria | Lead |
| DevOps | `.github/`, `scripts/`, this document | CI shape, release artifacts | Lead |

Small teams rotate roles, but the review gate is never self-approved: the Lead
reviews every change that lands on `main`.

## 2. Task board flow

1. A task states the outcome, the acceptance criteria and the write scopes it
   expects to touch. Work that depends on another task is marked `blocked_by`.
2. The owner claims the task at its current revision. One owner per task.
3. Work stays inside the declared scopes. A change that must cross a scope is
   agreed with the Lead before it is made.
4. When a task changes materially its owner updates the description; readers
   always trust the latest revision.
5. The owner completes the task at a fresh revision, and only after the
   definition of done below is satisfied.

The board is the single record of work in flight. Chat and notes can move
faster, but they do not replace the board.

## 3. Definition of done

A change is done when all of the following hold:

- it builds and runs from a clean checkout on Windows, Linux and macOS;
- `cargo fmt --all -- --check` reports no diff;
- `cargo clippy --workspace --all-targets -- -D warnings` reports no warning;
- `cargo test --workspace` passes; new public behaviour has tests, and protocol
  changes have round-trip tests;
- documentation is updated in both editions, per [LANGUAGE.md](LANGUAGE.md);
- no tokens, local absolute paths, proxy settings or account details appear in
  the diff;
- scripts stay idempotent and were verified by running them, not by reading
  them;
- the task board entry is completed at a fresh revision.

## 4. Review gates

1. Author self-check: run the three commands above and read the full diff.
2. Lead gate: the Lead runs formatting, clippy and tests again, reads the full
   diff, checks that the declared write scopes were respected and that the task
   description is honest.
3. Only after the gate passes does the Lead commit. Nobody commits or pushes
   while a gate is open.
4. `main` is squash-merged and must stay green; CI is the second gate on every
   push and pull request.
5. A failed gate returns the task to its owner with the failing command and its
   output on the task, not just a verdict.

## 5. Commit messages

Format:

    <type>: <one-line conclusion>

    - key conclusion
    - affected scope
    - relevant data (version, threshold, error code)

- `type`: `feat` `fix` `perf` `refactor` `docs` `test` `build` `ci` `chore`.
- The subject states the result of the change, never the process that found it.
- The body carries conclusions, constraints, blast radius and how the change
  was verified. It never carries reasoning transcripts, debug logs, tool
  output or a narrative of attempts; those belong in the task, not in history.
- One commit, one change: keep refactors and features separate so either can be
  reverted alone.

This repeats section 1 of [CONTRIBUTING.md](../CONTRIBUTING.md) and does not
relax it.

## 6. Release cadence

- Milestone releases ship when the exit criterion of M1-M4 in
  [03-roadmap.md](03-roadmap.md) is met, not on a calendar alone.
- Patch releases ship when a defect blocks users; there is no fixed train.
- Every release requires green CI on all three platforms and a completed Lead
  gate.
- Release steps: bump the workspace version in `Cargo.toml`, add the changelog
  entry, run `scripts/build.sh`, publish the binaries with `SHA256SUMS`, tag
  `vX.Y.Z`.
- Release notes exist in English and Chinese, like every other document.

## 7. Local development

- `scripts/dev-setup.sh` locates the Rust toolchain, prints the versions in use
  and verifies that the workspace resolves with `cargo metadata --offline`.
- `scripts/build.sh` builds `corvusd` and `corvus` in release mode into `dist/`
  and writes `dist/SHA256SUMS`.
- Local builds use `--offline` because this machine keeps a warm cargo cache
  and has no general network access. CI installs from the network normally; the
  two environments differ only in that flag, and CI is the authority on a clean
  build.
- CI runs the same three gates on `ubuntu-latest`, `windows-latest` and
  `macos-14`; see [.github/workflows/ci.yml](../.github/workflows/ci.yml).
