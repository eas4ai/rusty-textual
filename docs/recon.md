# Recon: rusty-textual (2026-09-24)

Path A recon for sudus: this repository has no `docs/spec/` yet. It records what
exists before any question is asked. Measured at `main` `0d593d4`.

Status words:

- **Exists**: observed in code or in a command run during this recon.
- **Documented**: stated in a doc or config; not checked against code.
- **Contradicted**: a doc or config says one thing; code or a run shows another.
- **Unverified**: not checked; the reason is given.

Commands run during this recon (logs in the session scratchpad, not committed):

| Command | Result |
|---|---|
| `script -qefc 'cargo test --lib -- --test-threads=1' /dev/null` | 2508 passed, 0 failed, 3 ignored |
| `setsid cargo test --lib -- --test-threads=1 < /dev/null` (no controlling TTY) | 2508 passed, 0 failed, 3 ignored |
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets -- -D warnings` (run in error: `-D warnings` is forbidden by the developer's hard rule; superseded by strict clippy below) | clean |
| `cargo build --examples` | success |
| `script -qefc 'cargo test --tests --no-fail-fast -- --test-threads=1' /dev/null` (before the Python removal below) | 116 binaries: 1242 passed, 115 failed, 3 ignored. Failures: `pty_interactive` 114 (Python paths absent), `pty_parity` 1 (`docs_sparkline_colors`) |
| After the Python removal: `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings` (same error as above) | clean; clean |
| Strict clippy, the required form: `cargo clippy --all-targets -- -W clippy::pedantic` (counted from `--message-format=json`) | root crate: 8339 warnings (top: `must_use_candidate` 1758, `return_self_not_must_use` 1228, `cast_possible_truncation` 629, `doc_markdown` 472); `docs/examples` `textual-docs-widgets`: 1113 (`unreadable_literal` 1000); `sparkline_colors` after the fix: 0 |
| After the Python removal: `script -qefc 'cargo test --test visual_parity --test visual_parity_interactive -- --nocapture --test-threads=1' /dev/null` | `visual_parity`: 87 discovered, 87 PASS; `visual_parity_interactive`: `button_focus` PASS |
| `cargo test --test pty_parity docs_sparkline_colors` (run twice) | fails both times |

Changes made during recon, at the developer's direction (2026-09-24):

- `.github/workflows/release.yml`: only `create-release` runs on a `v*` tag; tests and crates.io jobs are commented out.
- `.github/workflows/ci.yml`: manual `workflow_dispatch` only; `pty_interactive` removed from the exclusion list.
- Python Textual is no longer needed ("The python version should not be necessary for this project"): `tests/pty_interactive.rs` deleted; the `REGEN_*` Python branches removed from `tests/visual_parity.rs` and `tests/visual_parity_interactive.rs`; `visual_parity` now discovers cases from its committed goldens instead of `../textual`. `tools/parity/` is kept as optional tooling. `README.md` updated to match.
- `scripts/record_demo.py` writes a marker per example; new `scripts/demo_frames.py` turns a cast into one PNG per example (Pillow via Homebrew).
- The `dictionary` example, its `pty_parity` case and golden were removed ("We can make a different one").
- `docs_sparkline_colors` fixed: the docs example uses Python's literal `3.14` again (see section 4).

## 1. Identity and manifests

| Claim | Status | Citation |
|---|---|---|
| The crate is `rusty-textual` 1.1.0, edition 2024, MSRV 1.85.0, MIT. | Exists | `Cargo.toml:2-7` |
| `unsafe` code is forbidden. | Exists | `Cargo.toml:60` |
| Runtime stack is `rich-rs` 1.2.2, `crossterm` 0.28 and `tokio` 1.43. | Exists | `Cargo.toml:27-29` |
| Proc macros live in `textual-macros` 1.1.0, a path dependency with its own `Cargo.toml`. It is not a workspace member. | Exists | `Cargo.toml:41`, `textual-macros/Cargo.toml` |
| Macro expansions resolve the crate path even when a consumer renames the dependency. | Exists | `textual-macros/src/crate_path.rs:19-26` |
| The crate was published as `textual` 1.1.0 and is not yet published as `rusty-textual`. | Documented | `README.md` Attribution note |
| The code runs on Linux, macOS and Windows. | Unverified | `README.md` Compatibility; `ci.yml` (manual only) targets `ubuntu-latest` only |
| MSRV 1.85 builds. | Unverified | `ci.yml` uses `dtolnay/rust-toolchain@stable` only; this recon used rustc 1.95.0 |
| `docs/examples/` is a separate 27-member Cargo workspace of ported Python doc examples. | Exists | `docs/examples/Cargo.toml` |
| The `docs/examples/` workspace builds. | Unverified | Not built in this recon; commit `4cfd7e9` claims the rename fallout is fixed |

## 2. Entry points and public API

| Claim | Status | Citation |
|---|---|---|
| Library root declares 32 public modules (plus private `error`), a `prelude`, and re-exports `App`, `Pilot`, `Screen`, `Theme`, the `Reactive`, `on` and `widget` macros. | Exists | `src/lib.rs` |
| App entry points: `run_sync_with_output`, `run_test`, and sized/async variants. | Exists | `src/textual_app.rs:1415-1567` |
| `TextualApp::take_exit_output` hook. | Exists | `src/textual_app.rs:342` |
| `Pilot::click(selector)` drives a headless app. | Exists | `src/runtime/pilot.rs:97` |
| `App::load_stylesheet`, `load_stylesheet_file`, `watch_stylesheet`. | Exists | `src/runtime/mod.rs:4184-4197` |
| `App::set_theme_by_name`, `App::register_theme`, `available_theme_names`. | Exists | `src/runtime/mod.rs:3683,3720`, `src/theme.rs:494` |
| Built-in themes include textual-dark, textual-light, nord, gruvbox, dracula, tokyo-night, monokai, solarized-light. | Exists | `src/theme.rs:586-816` |
| `App::suspend` returns a `SuspendGuard`. | Exists | `src/runtime/mod.rs:2634` |
| All widgets the README lists exist as `pub struct`, plus `DateInput`. | Exists | `src/widgets/` (checked by name) |
| `CommandPalette` is a widget. | Contradicted | No `pub struct CommandPalette`; it is `CommandPaletteScreen` in `src/widgets/command_palette_screen.rs` |
| "Five layout modes: vertical, horizontal, grid, dock, absolute." | Contradicted (wording) | `Layout` enum has three values (`src/style.rs:1185-1189`); dock and absolute are separate `dock` and `position` properties (`src/layout/dock.rs`, `src/layout/mod.rs:1076`) |
| Debug env vars `TEXTUAL_DEBUG_{STYLE,LAYOUT,INPUT,RENDER}_FILE` and `TEXTUAL_DEBUG_STYLE_FILTER`. | Exists | `src/debug.rs:117-119`, `src/css/selectors/debug.rs:58`, `src/runtime/event_loop.rs:257` |
| Nine runnable examples: calculator, code_browser, diff, five_by_five, json_tree, markdown, merlin, readme_screens, reminder. | Exists | `examples/*/main.rs`; README lists all but `reminder`. `dictionary` was removed on 2026-09-24 by developer ruling (its results pane stayed empty; a different example will replace it) |

## 3. Data

| Claim | Status | Citation |
|---|---|---|
| The library keeps no persistent user data. Stored data is test fixtures and goldens. | Exists | `tests/pty_parity/{golden,golden_styled,golden_styled_interactive,fixtures}`, `tests/snapshots/` |
| All goldens were recorded from Python Textual and are now fixed references. No test runs Python. | Exists | `tests/pty_parity.rs:6,156,168` (provenance comments); `tests/visual_parity.rs` and `tests/visual_parity_interactive.rs` headers. Regenerating from Python is possible only through the optional `tools/parity/gen-python-goldens.sh` |
| The optional parity tools still hard-code the original author's interpreter `/tmp/textual-venv/bin/python` as a default (overridable). | Exists | `tools/parity/gen-python-goldens.sh:22`, `tools/parity/scoreboard-docs.sh:22` |
| The sibling checkout `../textual` (Textual 8.2.8, `84674538f`) is present but no longer used by any test. | Exists | `../textual/pyproject.toml:3`; no `../textual` path left in `tests/*.rs` code (comments only) |
| The handoff recorded `../textual` at `06dbeef4b`. | Contradicted | `docs/plans/handoff.md`; checkout is now at `84674538f` |

## 4. Tests and harnesses

| Claim | Status | Citation |
|---|---|---|
| 115 integration test files plus in-source unit tests (116 before `pty_interactive.rs` was deleted). | Exists | `tests/*.rs` |
| Library unit tests: 2508 pass, 3 ignored. | Exists | run above |
| "Unit tests need a real TTY" (driver `initialize()` fails with `WouldBlock` headless). | Contradicted | The lib suite passed with no controlling TTY (run above). Source of the claim: deleted `KNOWN_GAPS.md` (`git show 7fb011c^:KNOWN_GAPS.md`), `docs/plans/handoff.md`. Integration bins without a TTY: Unverified |
| The 3 ignored lib tests are the translucent-modal render tests that need a truecolor profile. | Documented | deleted `KNOWN_GAPS.md` "Tracked correctness follow-ups" |
| `visual_parity` holds 87 cases, all passing. | Exists | 87 discovered, 87 PASS (run above); `PASSING` list and `tests/pty_parity/golden_styled/` hold the same 87 names |
| `visual_parity` and `visual_parity_interactive` test whatever docs-example binaries are already built. They do not rebuild them, and they silently skip a case whose binary is missing. | Exists | `tests/visual_parity.rs` `discover()`; `tests/visual_parity_interactive.rs` "SKIP (no bin)". The binaries were fresh for this recon's run (built 2026-09-24 05:47, after HEAD) |
| `pty_parity` holds 184 cases (185 before `dictionary_initial` was removed; README said "around 180", handoff said 186/186). | Exists | `tests/pty_parity.rs:57` (`CASES`); 184 golden files; `manifest_matches_golden_files` passes |
| `pty_parity` passes all cases. | Exists (fixed 2026-09-24) | `docs_sparkline_colors` had failed consistently. Root cause: `4cfd7e9` replaced the example's divisor `3.14_f64` with `std::f64::consts::PI` to silence clippy's `approx_constant`, but Python's `sparkline_colors.py` divides by the literal `3.14`. Python's own sparkline algorithm reproduces the golden with `3.14` data and Rust's output with `PI` data, byte for byte, so the widget was correct. Fixed by restoring `3.14_f64` under `#[allow(clippy::approx_constant)]` with a comment (`docs/examples/widgets/examples/sparkline_colors/main.rs`). After the fix: `pty_parity` 185 passed, 0 failed; `visual_parity` and `visual_parity_interactive` pass |
| The `docs/examples` workspace is rustfmt-clean. | Contradicted | `cargo fmt --check --manifest-path docs/examples/Cargo.toml --all` reports diffs in 162 files. The root `cargo fmt --check` does not cover this separate workspace |
| `pty_interactive` is 108 passing plus 3 deliberate `#[ignore]`s. | Contradicted | Before deletion it ran 0 passed, 114 failed, 3 ignored here: it hard-coded the original author's `/tmp/textual-venv/bin/python` and `/mnt/shares/Marcos/...` checkout (added in `50cc0f1`). Deleted on 2026-09-24 by developer ruling |
| `interactive_parity` needs `../textual`. | Contradicted | It is a headless `Pilot` test with no Python or `../textual` reference (`tests/interactive_parity.rs`); `ci.yml` comment corrected |

## 5. CI and release

| Claim | Status | Citation |
|---|---|---|
| CI runs automatically on push to `main` and on pull requests. | Exists (disabled) | The developer ruled on 2026-09-24 not to run CI. `.github/workflows/ci.yml` now triggers only on `workflow_dispatch` (manual); the push and pull-request triggers are commented out |
| `ci.yml` defines a `pty-parity` job and a `test` job (lib plus headless integration bins under `script`, single-threaded, excluding five bins). | Exists | `.github/workflows/ci.yml` (manual runs only) |
| CI runs `clippy -D warnings` and `fmt --check` as blocking jobs. | Contradicted | Stated in `docs/plans/2026-09-22-parity-closeout.md` Validation Plan and `docs/plans/handoff.md`; `ci.yml` has no clippy or fmt step. The `-D warnings` form also breaks the developer's hard rule (strict clippy only, never `-D warnings`) |
| "`clippy --all-targets` clean under strict lints". | Contradicted | Commit `0f05ee1` message. Strict clippy (pedantic at warn level) reports 8339 warnings on the root crate (run above) |
| `visual_parity` runs on CI. | Contradicted | Excluded in `ci.yml`; it needs the `docs/examples` workspace built, which CI does not do. Local only |
| A `v*` tag push creates a GitHub source release and runs nothing else. It runs no tests and does not publish to crates.io. | Exists | `.github/workflows/release.yml`: only `create-release` is active; `test`, `pty-parity`, `publish-macros`, `package` and `publish` are commented out. The developer ruled on 2026-09-24: source releases only, no crates.io release, no CI |
| The commented-out crates.io jobs still name the old package `textual`. | Exists | `release.yml` header note; `cargo pkgid textual` fails with "did not match any packages". The jobs must be renamed to `rusty-textual` before they are re-enabled. Before the change, a tag push would have published `textual-macros` and then failed at `package` |
| `release.yml` describes the crate as "1.0.0-dev / WIP". | Contradicted | `release.yml:20`; version is 1.1.0 |
| `textual-macros` trusted publishing works. | Unverified | Moot while crates.io publishing is disabled. Deleted `KNOWN_GAPS.md` says the OIDC token is valid for `textual` only; not checked against crates.io |
| `.github/release.yml` groups GitHub release notes by label. | Exists | `.github/release.yml` |

## 6. Scripts and tools

| Claim | Status | Citation |
|---|---|---|
| `tools/run-doc-example.sh <group> <name>` runs a ported doc example, indexed by `tools/doc_examples_index.toml`. | Exists | `tools/run-doc-example.sh`, `README.md` |
| Parity tooling: golden generator, manual-verify, scoreboard, tmux guards. | Exists | `tools/parity/` |
| `tools/bench_runtime.sh`, `tools/gen-doc-example-stubs.sh`. | Exists | `tools/` |
| `scripts/record_demo.py` drives the 9 showcase examples in a PTY and writes an asciicast; `agg` turns it into a GIF and ffmpeg extracts PNG frames for visual layout checks. Muse used this to fix the example layouts. | Exists | `scripts/record_demo.py` docstring; commit `7c35661`; run during this recon (9 segments; frames viewed). Since 2026-09-24, `record_demo.py` writes a marker per example and `scripts/demo_frames.py` turns a cast into `NN-<example>.png` in one command (Pillow via `brew install pillow`) |
| The `dictionary` example showed its lookup result. | Contradicted (example removed 2026-09-24) | Recorded frames after typing "rust" and Enter show an empty results pane in 2 of 2 recordings, though the simulated lookup takes 80 ms (`examples/dictionary/main.rs:188-196`) and the recorder waited 3 s. Cause Unverified |
| `scripts/demo_snapshots.py` writes Python-vs-Rust SVG snapshots. | Contradicted | It imports `textual._doc` from `../textual` (conflicts with the no-Python ruling) and runs `cargo run --example buttons`, which no longer exists |

## 7. Non-spec docs

| Claim | Status | Citation |
|---|---|---|
| `README.md` is current for the rename and lists API, widgets, examples, debug vars. | Exists | `README.md` (rewritten in `7fb011c`); exceptions in section 2. Its test and parity paragraphs were updated on 2026-09-24 to say goldens are recorded from Python and tests do not need Python |
| `CHANGELOG.md` records changes since 1.1.0. | Contradicted | `[Unreleased]` is empty (`CHANGELOG.md:8`); 68 commits landed after `922cf93`, including the rename and remediation PR-10 to PR-19 |
| `ROADMAP.md` and `KNOWN_GAPS.md` exist. | Contradicted | Both deleted in `7fb011c` ("drop stale docs"); still referenced by `docs/plans/2026-09-22-parity-closeout.md` and `docs/plans/handoff.md` |
| The dispatch-model RFC is decided and tracked in Git. | Exists | `docs/plans/2026-09-23-dispatch-model-rfc.md` (tracked; cited by `README.md`) |
| The adversarial review, remediation PR split, parity-closeout plan and handoff are untracked. | Exists | `git status` |
| The remediation PR split (PR-01 to PR-19) has landed. | Exists | commits `1531226` to `e5d6c12`; RFC follow-ups P-A to P-G in `31cf029` to `fc24a33` |
| The parity-closeout plan (P0 to P7) is awaiting approval. | Contradicted in part | `docs/plans/handoff.md` says awaiting approval; P2 `App::suspend` has since landed (`8f65c2a`), and the plan cites deleted `KNOWN_GAPS.md` / `ROADMAP.md` |

## 8. Open items carried from the deleted KNOWN_GAPS.md

`KNOWN_GAPS.md` was the only backlog. It was deleted in `7fb011c`, so these
items now have no home outside Git history. Each was re-checked in code.

| Item | Status | Citation |
|---|---|---|
| Inline terminal render mode (`run(inline=True)`): no inline region, alt-screen suppression or height clamp. | Exists (still missing) | No inline render path in `src/driver/`; only the `:inline` CSS pseudo flag (`src/runtime/mod.rs:728`) |
| OS-level SIGCONT resume: no `SignalResume` handling, so `suspend_process` never publishes a resume. | Exists (still missing) | No `SIGCONT`/`SignalResume` in `src/`; `SIGTSTP` only (`src/runtime/mod.rs:892,2633`) |
| DataTable `--header-cursor` / `--fixed-cursor` classes are declared but not consumed by render. | Exists (still missing) | Declared only in `component_classes` (`src/widgets/data_table.rs:2301,2303`) |
| `loading` is not consulted by the focus chain or hit-test. | Exists (still missing) | Focus chain checks `disabled` only (`src/runtime/helpers.rs:246,291`) |
| No `get_loading_widget()` override hook. | Exists (still missing) | No match in `src/` |
| `Event::Unmount` dispatch to already-pruned nodes may be dead. | Unverified | Deleted `KNOWN_GAPS.md` PR-14 follow-up; needs a parity probe |
| Toast crossing screens gets a fresh countdown; cross-tree widget-timer hazard; Tooltip screen-layer escape. | Unverified | Deleted `KNOWN_GAPS.md` "Per-screen toast racks" |
| LAB shade tokens diverge up to ~42/channel. | Unverified | Deleted `KNOWN_GAPS.md` |
| `styles/layout` 2-row vertical drift; `Constrained` min-only/max-only chrome under-report; `scroll_view` parallel scrollbar path; Pilot key-cascade duplicate; `intrinsic_wrapped_height` trailing newlines; `MarkdownViewer` vs `VerticalScroll` selectors. | Unverified | Deleted `KNOWN_GAPS.md` |
| Intentional divergences: nearest-wins nested `layers`; Python-only startup crash; no message MRO; no `BINDINGS` inheritance; deferred-phase watchers; modern theme palette. | Documented | Deleted `KNOWN_GAPS.md`; nearest-wins pinned by `runtime::render::tests::nested_layers_declarations_are_nearest_wins` |
| Former divergences now changed: `key_<name>` fallback added (`9cb755b`); private `_watch_`/`_validate_` hooks added (`1d3d533`); capture phase kept by RFC R1. | Exists | commits cited; `docs/plans/2026-09-23-dispatch-model-rfc.md` |

## 9. Recent history

The repository has 1112 commits, from root `834ceee` (2026-02-03, "docs: add
rich-rs integration contract") to `0d593d4` (2026-09-24). There are no tags, so
releases 1.0.0 to 1.1.0 are marked only in `CHANGELOG.md`. 68 commits (merges
included) landed after the 1.1.0 docs commit `922cf93` (2026-07-16):

- 2026-09-22: adversarial-review remediation PR-01 to PR-19, each merged from a `fix/pr-NN-*` branch (`1531226` to `e5d6c12`).
- 2026-09-22 to 09-23: dispatch-model RFC follow-ups P-A to P-G and R7 (`31cf029` to `971c891`).
- 2026-09-23: crate renamed to `rusty-textual`; README rewritten; `ROADMAP.md` and `KNOWN_GAPS.md` dropped (`7fb011c`).
- 2026-09-23 to 09-24: calculator, merlin, diff and reminder examples; `DateInput` widget; strict-clippy cleanup; example layout fixes; App layout hook; PTY demo recorder; repository URL update; Link tests stop opening browser tabs (`6d0c10d` to `0d593d4`).

## 10. Findings since the recon (2026-09-24 to 2026-09-25)

Found during the strict-clippy cleanup and the long-function splits
(`79f6864e` to `e6d2f4c7`). Measured at `main` `e6d2f4c7`. The splits kept
behavior unchanged on purpose. The combinator panic was fixed during the
cleanup. The other five defects were fixed on 2026-09-25: the DirectoryTree
symlink gap, the `AppSimulateKey` class changes, the headless action-map gap,
the dropped style-animation requests and, on Linux only, the terminal
hang-up spin.

### 10.1 Defects and gaps

| Claim | Status | Citation |
|---|---|---|
| A CSS child-combinator chain longer than the ancestor stack (`A > B > C` where `B` is the top ancestor) panicked with an index out of bounds. It now does not match. | Exists (fixed in `794f7eb4`) | Guards: `src/css/selectors/matching.rs:134` (`rule_specificity`) and `src/runtime/event_loop.rs:1531` (`rule_matches_snapshot_chain`). Tests: `matching.rs:217`, `event_loop.rs:7559` |
| `DirectoryTree` lists a symlink to a directory as a file, so the user cannot expand it. Python follows the link. | Exists (fixed 2026-09-25) | Child entries used `DirEntry::file_type()`, which does not follow symlinks, in both the sync and the async listing (`src/widgets/directory_tree.rs:553`, `src/runtime/tasks.rs:508` at `e6d2f4c7`). The root node uses `Path::is_dir()`, which does (`:37`). Python uses `path.is_dir()` for both (`../textual/src/textual/widgets/_directory_tree.py:457-467`). Present since `7628e659`. Both listings now use `Path::is_dir()`; tests `read_children_lists_a_symlinked_directory_as_a_directory` and `read_directory_request_lists_a_symlinked_directory_as_a_directory` |
| An app whose terminal (pty) closes without a SIGHUP keeps running at 100% CPU. This happens when the app has no controlling terminal, for example under `scripts/record_demo.py`. | Exists (fixed on Linux 2026-09-25) | Observed 2026-09-24 on `79f6864e`: each recording left eight example processes at full CPU (`c7cfb55f` message; that commit makes the recorder kill the whole process group). Root cause, confirmed 2026-09-25 with `strace` on `calculator` under a pty with no controlling terminal: after the pty closes, crossterm's Unix input source calls `read(0) = 0` about 87,000 times a second and never returns from `event::poll`. Its read loop treats 0 bytes and non-`WouldBlock` errors as "try again" (crossterm 0.28.1 and 0.29.0 `src/event/source/unix/mio.rs`; upstream issue crossterm-rs/crossterm#793, open since 2023; PRs #1067 and #1116 unmerged). Python Textual's input thread gets an `OSError` from `os.read` and the app exits (`../textual/src/textual/drivers/linux_driver.py:403-463`). Fix: `src/driver/hangup.rs`, a thread that waits in `poll(2)` for the terminal's hang-up and raises SIGHUP, started and stopped with the driver. After the fix the same run exits within 0.5 s. Linux only: macOS `poll(2)` does not support devices. Tests `fires_when_the_terminal_hangs_up`, `stops_without_firing_when_dropped` |
| A key sent through `AppSimulateKey` lost the CSS class changes that its binding's action staged on its `EventCtx` (reachable through `WidgetCtx::event_ctx_mut()`). The live and headless key paths apply them. The usual `WidgetCtx::add_class` goes through the command queue and was not affected. A Footer key click and the `app.simulate_key` action both post `AppSimulateKey`. | Exists (fixed 2026-09-25) | At `e6d2f4c7`: `dispatch_simulated_key_binding` passed each action's `EventCtx` to `merge_ctx_into_runtime_pass` (`src/runtime/event_loop.rs:614,624,637`), which left class changes on the context (`:545`); the context was then dropped. The live path (`:3505,3530`) and the headless path (`:5720,5732,5745`) use `outcome_from_action` (`:2217`), which keeps them. `WidgetCtx::add_class`: `src/runtime/widget_ctx.rs:172`. Posters: `src/widgets/footer.rs:852`, `src/textual_app.rs:969`. Now `merge_ctx_into_runtime_pass` takes the class changes too; test `app_simulate_key_keeps_class_ops_staged_by_the_binding_action` |
| In headless runs (`Pilot`), a key bound with `App::bind_key` to `CopySelectedText` or `HelpQuit` skipped the app-level handling (copy the app's text selection, or show the quit hint) and only reached widgets as `Event::Action`. The live loop and `AppSimulateKey` run the app-level handling. The default `ctrl+c` was not affected: for a `TextualApp`, the screen's `copy_selected_text` binding handles it the same way in every mode. | Exists (fixed 2026-09-25) | At `e6d2f4c7`: headless `headless_action_map` (`src/runtime/event_loop.rs:5758-5786`); live `live_action_map_fallback` (`:3598-3605`); simulated `dispatch_simulated_action_map` (`:655-675`). `App::bind_key`: `src/runtime/mod.rs:4476`; the `ctrl+c` binding: `src/textual_app.rs:1062`. The headless fallback now shares `copy_selected_text_or_help_quit` with the live loop; tests `bind_key_help_quit_and_copy_selected_text_show_the_quit_hint` and `ctrl_c_with_no_selection_shows_the_quit_hint` |
| `dispatch_event_auto` dropped style-animation requests that the root widget stages in its key-capture, event-bridge and app-action hooks (for example a `TextualApp` key handler that calls `ctx.animate_style`). It kept the other requests from those hooks (messages, animations, workers, recomposes, class changes). Live and headless runs share this code, so both were affected. No example used this path. | Exists (fixed 2026-09-25) | At `e6d2f4c7`: `prepend_ctx_effects` and `append_ctx_effects` copied everything except style-animation requests (`src/runtime/event_loop.rs:2237,2275`), and `dispatch_event_auto` (`:7217`) then dropped the contexts. A root key capture that handled the key returned through `outcome_from_action`, which kept them. Both helpers now merge them; test `dispatch_event_auto_keeps_style_animations_from_the_root_hooks` |
| `WidgetTree::apply_forwarded_seed` is public but nothing calls it. | Exists | `src/widget_tree.rs:795`. Its callers were removed with `Node` in `f0b4c684`. Its doc says so since `91deae05` |
| On the development machine, some build and test failures came from hardware instability, not code: kernel-logged segfaults on several different cores, a test binary with 26 zeroed bytes, and mold linker crashes. | Exists | `journalctl -k`, 2026-09-24 20:43 to 20:59. Each failure passed on a fresh build. Rule since then: rerun a failure on a fresh build and compare with the parent commit before debugging code |

### 10.2 Earlier rows that have changed

| Earlier claim | Now | Citation |
|---|---|---|
| Strict clippy: root crate 8339 warnings, `docs/examples` `textual-docs-widgets` 1113 (commands table and section 5). | 0 in the root crate, `docs/examples` and `textual-macros`. | Phases `616ad53b` to `22c8c0d4`; long-function splits `904fc026` to `fab1c060`. Counted from `--message-format=json` on 2026-09-25 |
| `cargo doc --no-deps` warnings (not measured in the recon). | 0 in all three workspaces. The root crate had 46. | `91deae05` |
| `docs/examples` is not rustfmt-clean (diffs in 162 files). | Clean. | `0d836f37`, `e6d2f4c7` |
| Library unit tests: 2508 passed, 3 ignored. | Full gate: the library and 115 integration binaries, 3761 passed, 3 ignored. Examples: 64 passed. `docs/examples`: 30 passed. | Last full gate run 2026-09-24 23:57 during the splits. Only doc comments and formatting changed after it |
| `CHANGELOG.md` `[Unreleased]` is empty. | It records the strict-clippy changes. | `CHANGELOG.md:8` |
| `scripts/record_demo.py` stops only cargo after each example. | It kills the example's whole process group. | `c7cfb55f` |
