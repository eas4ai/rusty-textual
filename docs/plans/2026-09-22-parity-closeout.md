## Goal

Close the remaining measured gaps between `textual-rs` 1.1.0 and Python Textual to reach full 1:1 behavior parity (idiomatic Rust API), working in the found port at `/home/shawn/workspace2/textual-rs/` and keeping `textual-rs-bak/` untouched as backup. Plan docs live in the port's own `docs/plans/`.

## Success Criteria

- Inline render mode and `App.suspend()` work; `how-to/inline01`, `inline02`, `clock`, and `guide/app/suspend` demos are unblocked.
- `DataTable` consumes the declared `--header-cursor` / `--fixed-cursor` component classes, with ported regression tests.
- `tests/pty_interactive.rs` is 111/111 (the 3 current `#[ignore]`s resolved or deliberately re-pinned), with `tests/visual_parity.rs` 87/87 and `tests/pty_parity.rs` 186/186 staying green.
- Unit tests run headless with no PTY wrapper; the 3 ignored translucent-modal render tests are un-ignored.
- CI runs `visual_parity` against a checked-out Python reference and `textual-macros` publishes without manual steps.
- No new intentional divergence lands without a `KNOWN_GAPS.md` entry.

## Context And Current Facts

- The found port is shippable: crate `textual` v1.1.0, edition 2024, `unsafe_code = "forbid"` (`Cargo.toml`); stack is `rich-rs` + `crossterm` 0.28 + `tokio` 1.43, not a from-scratch starter (`textual-rs-bak/` holds the empty Hello-world crate).
- `ROADMAP.md`: phases 0–5 and 7 done, 1.0–1.0.3 closed 18 confirmed bugs with ported regression tests, 1.1.0 delivered the extension story and structural parity (cross-screen access, component classes, Tree/OptionList key identity, TextArea document subsystem, keymap subsystem, fine-grained messages). Every fix is parity-gated (visual + pty_parity idle) with a ported regression test.
- `KNOWN_GAPS.md` (as of 1.1.0): zero open bugs; 3 interactive `#[ignore]`s are intentional divergences or the deferred inline feature; deferred beyond 1.1 are inline render mode and `App.suspend()`; 1.1.x follow-ups are DataTable header/fixed cursors, `textual-macros` trusted publishing, and CI `visual_parity` needing a Python-ref checkout.
- Tracked correctness follow-ups with no demo impact: unit tests need a real TTY (CI wraps the suite in `script -qefc`, single-threaded), `loading`/`disabled` not yet consulted by focus chain and hit-test, no `get_loading_widget()` override hook, toast sub-follow-ups (fresh countdown across screens, cross-tree timer hazard), LAB shade tokens diverging up to ~42/channel.
- Deliberate divergences stay: nearest-wins nested `layers`, Python-only startup crash, name-convention/`key_<name>` dispatch, message MRO, `BINDINGS`/reactive inheritance, theme tokens (`ROADMAP.md` W5, `KNOWN_GAPS.md`).
- Structural map (`ripwire --report` on the port): 709 files, 16202 symbols, acyclic; clusters around `src/widgets`, `src/runtime` + `widget_tree`, event dispatch, `textual_app`, stylesheet loading, and the CSS selector parser / style-layout core.
- Python reference is the sibling checkout (`../textual`, Textual 8.2.8, 247 `src` files, 410 test files); contributor checks there are `make test` (run the full test suite) and `make format`.

## Constraints And Non-goals

- Parity means behavior parity through an idiomatic Rust API (`Handle`, `ReactiveCtx`, typed messages, `#[on(Type)]`); no Python-API transliteration and no new widget families beyond parity demos.
- The recorded intentional divergences (layers nearest-wins, startup crash, dispatch/inheritance model) are constraints, not work items.
- LAB shade exactness is out of scope unless a fix falls out of other work.
- `textual-rs-bak/` is not modified by this plan.

## Key Decisions

- Continue the found port; do not restart from the starter. The port has three shipped releases, committed goldens, and measured harnesses; restarting would discard the extension story and component-class architecture.
- Keep the `rich-rs` + `crossterm` + `tokio` stack; no migration to another TUI framework. The render boundary (`rich-rs` Console renders segments, `FrameBuffer` diffs), metadata schema, and Tokio core runtime are load-bearing and golden-tested (`Cargo.toml`, `ROADMAP.md` Phase 0.5).
- Order by user-visible value first: inline mode + suspend, then DataTable cursors, then interaction semantics (`loading`/`disabled`, cover hook, toast timers), then harness/CI, then release chores. Deferred items already have this relative order in `KNOWN_GAPS.md`.
- Fix unit-test TTY dependence with a headless/mock driver rather than keeping the PTY wrapper forever; the wrapper stays until the mock driver lands.
- Keep the house rule: every fix parity-gated (visual + pty_parity idle) with a ported regression test; spec-first only for structural changes that need it.

## Recommended Approach

Run a phased closeout against `KNOWN_GAPS.md` as the backlog: one phase per gap class, each landing with its goldens and regression tests before the next starts. Inline mode is the keystone (it also unblocks `suspend`); the DataTable and interaction items ride the existing component-class and focus/hit-test seams; harness work (mock driver, `TEXTUAL_PY_REF`, un-ignores) closes last so live goldens validate it. Update `KNOWN_GAPS.md` and `ROADMAP.md` in the same phase that closes each item.

## Work Plan

- P0 — Scaffold: create `docs/plans/2026-09-22-parity-closeout.md` (this file); confirm `textual-rs-bak/` parity as restore point. Depends on: approval of this plan.
- P1 — Inline render mode (`src/driver`, `src/runtime`): inline render region, alt-screen suppression, height clamp; unblock `how-to/inline01`, `inline02`, `clock`. Depends on: P0.
- P2 — `App.suspend()` (`src/textual_app.rs`, driver teardown/restore): inline-subprocess context manager reusing the P1 teardown path; unblock `guide/app/suspend`. Depends on: P1.
- P3 — DataTable header/fixed cursors (`src/widgets`, component `render`): consume `--header-cursor` / `--fixed-cursor` deliberately as a behavior change with restyle goldens. Depends on: P0.
- P4 — Interaction semantics (`src/runtime` focus chain, hit-test, `Screen`/`App` cover): consult `state.loading` in focus and hit-test, add the `get_loading_widget()` override hook, give crossing toasts a creation instant. Depends on: P0; touches broad semantics, so lands behind P1–P3.
- P5 — Headless/mock driver for unit tests (`src/driver`, `src/runtime/event_loop.rs` tests): run the ~130 `initialize()`-based tests and the 3 ignored translucent-modal tests without a TTY; then drop the CI PTY wrapper. Depends on: P0; validates P1–P4.
- P6 — CI and release chores: `TEXTUAL_PY_REF` checkout so `visual_parity` runs blocking on CI; trusted publishing for `textual-macros`; un-ignore resolved interactive tests. Depends on: P5.
- P7 — Docs closeout: update `KNOWN_GAPS.md` (move items to closed), `ROADMAP.md` (1.2 milestone), and `CHANGELOG.md` per phase. Depends on: each phase as it lands.

## Validation Plan

- Per phase: focused `cargo test -p textual <phase_module>` plus the affected integration bins under `tests/`; expected evidence is green focused bins with no golden drift elsewhere.
- Parity gates per fix: `cargo test --test visual_parity` (87/87 must hold, then grow), `cargo test --test pty_parity` (186/186 must hold, then grow), `cargo test --test pty_interactive -- --include-ignored` to confirm the 3 ignores resolve down to the deliberate set.
- Full suite until P5 lands (TTY workaround, from `KNOWN_GAPS.md`): `script -qefc 'cargo test -- --test-threads=1' /dev/null`; after P5 the same command must pass bare as `cargo test --all-targets -- --test-threads=1`.
- Lints: `cargo clippy -- -D warnings` and `cargo fmt --check` blocking per phase (matches the port's GitHub Actions jobs).
- Manual check that cannot be automated: run the real `clock` and `inline01` demos in a live terminal against real Python for P1–P2, since static harnesses previously passed while live clocks were dead.

## Risks / Rollback

- Live arena tick breadth and toast/focus changes are the widest behavioral risks; a live/pty golden drift points at P4 first, then the tick path. Roll back per phase (each phase reverts independently; goldens are committed so drift is visible).
- Inline teardown bugs can leave the terminal in a bad state; the P1 harness must assert restore on panic paths, or P2 `suspend()` inherits the hazard.
- `textual-macros` publishing and CI reference checkouts are external-service dependent; they degrade to manual steps without blocking code phases.
- Highest-risk validation step is the P5 mock-driver cutover: every previously PTY-wrapped test changes execution environment at once.

## Open Questions

None. Naming and target were confirmed in-session (found port is the target, `-bak` is backup); remaining unknowns are tracked in `KNOWN_GAPS.md`, not open plan questions.

## Sources

- https://docs.rs/crossterm/latest/crossterm/
- https://docs.rs/tokio/latest/tokio/
