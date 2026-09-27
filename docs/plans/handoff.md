# Session handoff — textual-rs parity closeout (2026-09-22)

## Status: plan presented, awaiting approval — do not implement yet

A parity-closeout plan for the found port is written and presented. The next
session must get an explicit `Approve` before touching code; on
`Request changes`, revise and re-present the full plan; on `Cancel`, stop.

## What happened

- Goal started as "1:1 parity port of Textual to Rust in `textual-rs`", then the
  target turned out to be an existing shipped port (crate `textual` v1.1.0,
  `rich-rs` + `crossterm` + `tokio`), not an empty starter. The empty starter
  was left alone at `textual-rs-bak/`.
- Oriented with `ripwire --report` (709 files, 16202 symbols, acyclic; clusters:
  `src/widgets`, `src/runtime` + `widget_tree`, event dispatch, `textual_app`,
  stylesheet loading, CSS selector parser) and grounded the plan in
  `ROADMAP.md` + `KNOWN_GAPS.md` (1.1.0 shipped, zero open bugs).
- Plan file (canonical, presented verbatim in-session):
  `docs/plans/2026-09-22-parity-closeout.md` — phases P0–P7: inline mode,
  `App.suspend()`, DataTable header cursors, interaction semantics,
  headless/mock driver, CI + `textual-macros` publishing, docs closeout.

## Repo stamps (measure point)

- `textual-rs` at `922cf93+dirty` ("docs: update README and KNOWN_GAPS for
  1.1.0"); `+dirty` = untracked `docs/plans/` (the plan file above — new
  session: keep it, do not commit unless asked).
- `../textual` at `06dbeef4b`, untracked `AGENTS.md` (separate earlier task,
  complete; no action needed).
- `textual-rs-bak/` is the untouched Hello-world starter — restore point only.

## Key facts for the next session

- Stack is `rich-rs` + `crossterm` 0.28 + `tokio` 1.43 (`Cargo.toml`); no
  framework migration is planned or wanted.
- House rule: every fix parity-gated (visual + pty_parity idle) with a ported
  regression test. Harnesses: `visual_parity` 87/87, `pty_parity` 186/186,
  `pty_interactive` 108/108 + 3 deliberate `#[ignore]`s.
- Full suite needs a TTY until the P5 mock driver lands:
  `script -qefc 'cargo test -- --test-threads=1' /dev/null`; lints
  `cargo clippy -- -D warnings`, `cargo fmt --check`.
- Intentional divergences (not work items): nearest-wins nested `layers`,
  Python-only startup crash, dispatch/inheritance model (`KNOWN_GAPS.md`).
- Gotcha learned: static harnesses once passed while the live stopwatch clock
  was dead — P1/P2 require live-terminal checks against real Python.

## First actions on resume

1. Read `docs/plans/2026-09-22-parity-closeout.md`, then `KNOWN_GAPS.md`
   "Deferred beyond 1.1" + "1.1.x follow-ups".
2. If approved, start at P1 (inline render mode, `src/driver` + `src/runtime`).
