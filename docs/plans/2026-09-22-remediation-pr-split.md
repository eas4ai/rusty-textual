# Remediation PR split — textual-rs adversarial review follow-up (2026-09-22)

## Goal

Remediate every actionable finding in
`docs/plans/2026-09-22-adversarial-review.md` as a sequence of small,
upstream-ready PRs against `eas4ai/textual-rs` (`main`), one fix per PR, each
shippable and revertable on its own.

## Success Criteria

- Each review §1 item (BLOCKER/BUG/FUCKUP) has a merged PR or an explicit
  deferral recorded in this file.
- No PR mixes unrelated fixes; each PR touches one cohesive surface and adds
  its regression test in the same diff (house rule: parity-gated with a ported
  regression test).
- CI (`pty-parity` + `test` jobs in `.github/workflows/ci.yml`) is green on
  every PR; the cannot-fail harness cluster is honest before behavior fixes
  land that would otherwise go unproven.
- The dispatch-model architecture (§3 structural items) has a recorded design
  decision (RFC), not drive-by changes.

## Context And Current Facts

- Stamp: `922cf93`, branch `main`, remote `origin`
  `https://github.com/eas4ai/textual-rs` (fetch+push). Working tree dirty only
  with untracked `docs/plans/`, `.claude/`, `AGENTS.md`, `CLAUDE.md`.
- Findings source: `docs/plans/2026-09-22-adversarial-review.md` (§1–§9).
  Pivotal claims re-verified in-tree during planning: pump cap
  (`src/runtime/routing.rs:510`), `set_items` panic
  (`src/widgets/option_list.rs:415`), pending harness case
  (`tests/visual_parity_interactive.rs:39`), tautological leak assert
  (`tests/pty_interactive.rs:1005`), CSS swallow (`src/screen.rs:575`), `u8`
  specificity (`src/css/selectors/matching.rs:76`), part-sum
  (`matching.rs:140`).
- CI reality: `pty-parity` job runs `cargo test --test pty_parity` (blocking,
  headless-safe); `test` job runs lib + headless integration under `script`
  single-threaded, skipping the six Python/PTY-dependent harnesses
  (`pty_parity`, `pty_interactive`, `visual_parity`,
  `visual_parity_interactive`, `interactive_parity`, `click_actions_pty`).
  No clippy/fmt job in CI — run `cargo clippy -- -D warnings` and
  `cargo fmt --check` locally per PR.
- Full local suite needs a TTY until the mock driver lands:
  `script -qefc 'cargo test -- --test-threads=1' /dev/null`.
- Nuance found while grounding: `dynamic_watch` (`pty_interactive.rs:856`)
  does not assert "must differ" as bare inequality — it fails with HARNESS
  BLIND when the apps look *identical*. Effect is the same (fixing parity
  turns it red), but PR-01 rewrites it as a straight parity assert rather
  than "removing a must-differ".

## Constraints And Non-goals

- One fix per PR; no unrelated cleanup, no renames beyond what the fix needs
  (the `commands.rs` NIT rides only if its PR touches that file — otherwise
  dropped).
- No behavior change without its regression test in the same diff.
- The §3 dispatch-model structural divergences (invented capture phase,
  missing bubble flags, no MRO walk, `@on` kwargs, focus-chain ordering,
  mouse capture/chains) are explicitly NOT in the PR sequence — they need the
  RFC decision first (see RFC below). Small contained §3 items with agreed
  behavior (MouseUp bubble carve-out) may go as ordinary PRs only after the
  RFC rules on the model.
- `KNOWN_GAPS.md` intentional divergences (nearest-wins layers, Python-only
  startup crash) stay untouched.
- Uninspected areas from review §8 (layout bodies, compositor paint/clip,
  TextArea/CommandPalette/containers, full API diff) are follow-up review
  scope, not silent inclusions.

## Key Decisions

- **Harness honesty first (PR-01).** Behavior fixes are unprovable while
  cannot-fail asserts exist, so the tautology/divergence asserts and the
  content-free snapshot are fixed before anything they cover.
- **Order by blast radius, not just severity.** Silent-wrongness fixes
  (pump cap, Unmount loss, CSS swallow) precede API additions (suspend,
  Pilot gaps); golden-affecting changes (cascade, dim blend) go late and
  alone so re-baselines are attributable.
- **Typed errors before fallible APIs (PR-11 → PR-12).** `DuplicateID` /
  `OptionDoesNotExist` / `StylesheetError` variants must exist before
  `set_items` can return them.
- **Driver capability flags split in two (PR-15a/b).** Paste+mouse defaults
  are low-risk; kitty/SYNC/resize touch live-terminal bytes and get their own
  PR with live-terminal verification.
- **Dim blend + pending flip ship together (PR-16).** The sole
  `visual_parity_interactive` case is pending on exactly that bg delta, so the
  fix and the flip prove each other; golden re-baseline stays inside this PR.
- **Branch convention:** `fix/pr-NN-slug` off `main`, one PR per branch,
  PR body cites the review section (e.g. "review §1.1"). Do not stack branches
  except PR-12 on PR-11.

## Work Plan

Status sweep (2026-09-22): PR-01–PR-08 landed on `main` + proposed upstream
(mrsaraiva #1–#8, noting #2 was stacked). Scope moves: unknown-sender
broadcast gate → RFC (pinned test + overlay justification); p1_tree_focus →
RFC (needs dispatch model). [UPDATE: arity + async shipped as PR-08b/PR-08c
via Rust-native mimicry per explicit direction — upstream mrsaraiva #9/#10.]

**PR-01 — Harness honesty (review §1.7, §7).** [LANDED] Files:
`tests/pty_interactive.rs` (~1005 weather05, ~856 dynamic_watch),
`tests/snapshots.rs`, `tests/p1_tree_focus.rs:62`. Change: weather05 leak
axis asserts `!rust_leaks` (keep the echo assert); dynamic_watch becomes a
parity assert (value==30, blue bar on both); snapshots.rs points at real
behavior or is deleted; p1_tree_focus rewritten to not depend on Focus
bubbling. Test: the harnesses themselves (run both before/after to show the
old asserts passed vacuously and the new ones are live). Depends on: nothing.
First.

**PR-02 — Remove pump 1024 cap (review §1.1).** Files:
`src/runtime/routing.rs:510`. Change: remove `LIMIT`/drop-tail (match Python
uncapped loop); if a cap is wanted for DoS safety it must spill + warn, but
default is removal. Test: flood regression (enqueue >1024, assert zero drops).
Depends on: PR-01.

**PR-03 — Deliver Unmount per pruned node (review §2).** Files:
`src/widget_tree.rs:305` (+ caller in `event_loop.rs:6025`). Change: drain
Mount and Unmount, dispatch each to its node. Test: mount/remove subtree,
assert Unmount received per node. Depends on: PR-01.

**PR-04 — Cancel node workers on unmount (review §1.4).** Files:
`src/worker.rs:246`, unmount path (`event_loop.rs:6062`). Change: wire
`cancel_by_owner` into unmount (timers stay). Test: worker liveness —
unmounted node's worker stops. Depends on: PR-01.

**PR-05 — Attach gates (review §2).** Files: `src/widget_tree.rs:397`,
`src/runtime/routing.rs:609`. Change: `mount()` on dead parent refuses
(returns error/no-op, no Mount emitted); unknown-sender messages require
`is_attached` instead of broadcast. Test: both gates unit-tested. Small and
cohesive (both are "is_attached" semantics). Depends on: PR-01.

**PR-06 — Message folding parity (review §2).** Files:
`src/runtime/routing.rs:435`, `src/message.rs:112`. Change: coalescer folds
head-vs-next only; `replaceable` set shrinks to Python's five. Test:
folding unit tests incl. non-adjacent same-sender case. Depends on: PR-02
(same pump area, avoids conflict).

**PR-07 — Reactive `_watch_`/`_validate_` dispatch (review §2).** Files:
`textual-macros/src/reactive.rs:596/432`. Change: dispatch `watch_` and
`_watch_`, run `_validate_` then `validate_`. Test: macro expansion tests +
runtime watcher test. Depends on: PR-01.

**PR-08 — Reactive arity/async + set flags (review §2).** Files:
`textual-macros/src/reactive.rs:589`, `src/reactive.rs:39`. Change: 0/1/2-arg
and async watchers via call_next; `_set` applies bindings/toggle_class flags.
Test: watcher-signature matrix test. Depends on: PR-07 (same files).

**PR-09 — Selector matching correctness (review §4).** Files:
`src/css/selectors/matching.rs:140/76/54`, `parser.rs:440`. Change:
lexicographic (id,class,type) specificity (kills the `u8` overflow by
construction); unknown pseudo → never-match; `:even` un-inverted (fix the
pinning test at `matching.rs:292` in the same diff). Test: unit matrix incl.
`#a #b #c` vs `#d`, 10-class vs `#id`, `:foobar`, first-child `:even`.
Depends on: PR-01.

**PR-10 — Cascade ordering (review §4).** Files: `resolver.rs:61`,
`style.rs:2121`. Change: Python `styles.py:980` key order (user-normal above
default-important). Alone because it re-baselines goldens. Test: cascade unit
tests + full `visual_parity`/`pty_parity` re-baseline inside the PR.
Depends on: PR-09 (same area).

**PR-11 — Typed errors + CSS surfacing (review §1.8, §6).** Files:
`src/error.rs`, `src/screen.rs:575`, `src/textual_app.rs:1359`. Change: add
`StylesheetError` (+ `DuplicateID`, `OptionDoesNotExist` for PR-12); missing/
unreadable CSS path raises instead of silent unstyled render. Test:
bad-CSS_PATH startup test asserting the error. Depends on: PR-01.

**PR-12 — OptionList fallible API (review §1.2, §6).** Files:
`src/widgets/option_list.rs:415/369`. Change: `set_items` returns `Result`
(`DuplicateID`), `get_option` by id raising `OptionDoesNotExist` (keep the
index accessor under a new name if callers need it). Test: duplicate-id and
unknown-id tests. Depends on: PR-11. May stack branch on PR-11.

**PR-13 — Mutex poison tolerance (review §1.9).** Files: `src/theme.rs:482`
and the `screen`/`tabs`/`tabbed_content` `.lock().expect()/unwrap()` sites.
Change: mechanical `unwrap_or_else(|e| e.into_inner())` following the existing
`textual_app.rs:1359` pattern. Test: poison-recovery unit test. Depends on:
PR-01. Conflicts watch: keep rebased.

**PR-14 — Suspend + AwaitRemove (review §2).** Files:
`src/runtime/mod.rs:2389/1807`. Change: `App.suspend` context manager
(signals/stdio per Python `app.py:4718/4708`); async `remove` returning
`AwaitRemove`. Test: suspend/resume signal test; awaited-remove ordering test.
API addition, moderate size — one PR, no golden impact expected. Depends on:
PR-03 (lifecycle area).

**PR-15a — Paste + mouse defaults (review §5).** Files:
`src/driver/platform/posix.rs:19`, `src/driver/mod.rs:60`. Change: emit
DECSET 2004 (paste arrives as PasteEvent, not raw text); `enable_mouse=true`
default. Test: headless escape-sequence test for 2004; mouse-default unit
test. Depends on: PR-01.

**PR-15b — Kitty/SYNC/resize negotiation (review §5).** Files:
`src/driver/platform/posix.rs:60/161`, `runtime/mod.rs:854`,
`render.rs:636`, `event_loop.rs:2351`. Change: full kitty flag set +
`DISABLE_KITTY_KEY` gate (no unconditional Auto); SYNC via DECRQM query with
Apple Terminal exclusion, off until negotiated; in-band resize handling.
Live-terminal verification required (recall the dead-stopwatch lesson:
static harnesses alone don't prove this). Test: negotiation unit tests +
live check. Depends on: PR-15a (same files).

**PR-16 — Dim blend opt-in + pending flip (review §5, §1.7).** Files:
`src/render/mod.rs:217`, `tests/visual_parity_interactive.rs:39`. Change:
dim becomes opt-in tunable (FILTERS + DIM_FACTOR per Python) instead of
hardcoded always-on 0.66; flip `button_focus` pending→pass. Golden
re-baseline inside. Test: the flipped harness case itself. Depends on:
PR-15b (driver area settled first).

**PR-17 — Widget behavior corrections batch (review §2 widgets).** Files:
`data_table.rs` (enter-only select, page_up/down actions, cursor-fallback
scroll, `show_cursor`, `header_height`), `button.rs:487` (enter-only),
`checkbox.rs:171` (hidden binding, `toggle_button`), `list_view.rs:486`,
`tabs.rs:1113`, `tree/mod.rs:1207`. Change: one-line behavior alignments per
finding; `show_cursor`/`header_height` are small additive props in the same
PR. Test: per-widget keybinding/behavior tests. Cohesive as "keybinding +
small-prop parity". Depends on: PR-01.

**PR-18 — Input parity (review §2 widgets).** Files: `src/widgets/input.rs`
(`:831` bindings, `:263` props). Change: Python's ~25 bindings subset that
has portable meaning + `valid_empty`/`compact`. Separated from PR-17 because
25 bindings is its own review surface. Test: binding table test. Depends on:
PR-17 (same area, avoid conflict).

**PR-19 — App/Pilot API gaps (review §3).** Files: `src/textual_app.rs`,
`src/runtime/mod.rs`, `src/runtime/pilot.rs`, `screen.rs:241`. Change:
`open_url`/`bell`/`export`+screenshot, `exit()` carrying result/code/message,
Pilot `pause(delay)`/`wait_for_animation(s)`/`exit(result)`/mouse down-up +
double/triple-click hooks (behavior minimal, API present). Test: Pilot API
tests. Depends on: RFC ruling only where Pilot touches dispatch; otherwise
PR-01.

**RFC — Dispatch-model architecture (review §3 structural, §8 gaps).**
No code. Decide: bubble-only vs invented capture phase; per-message bubble
flags; MRO handler walk + `prevent_default` semantics; `@on` selector kwargs;
focus-chain ordering (`_focus_sort_key`, trap, disabled/visibility); Focus/
Blur posting model; mouse capture + click chains; `key_<name>` dispatch.
Rule on each, then spawn follow-up PRs. Runs in parallel with the sequence;
its PRs land last.

## Validation Plan

- Per PR: affected crate/harness tests (`cargo test --test <name>` or
  `--lib <filter>`), plus `cargo clippy -- -D warnings` and
  `cargo fmt --check`. House rule: ported regression test in the same diff,
  parity-gated (visual + `pty_parity` idle) where the surface renders.
- Milestones (after PR-06, PR-10, PR-16, end): full TTY suite
  `script -qefc 'cargo test -- --test-threads=1' /dev/null`.
- CI on each PR gives `pty_parity` (blocking) + lib/headless integration;
  Python-dependent harnesses (`pty_interactive`, `visual_parity*`) run locally
  against `../textual` for PRs that touch covered surfaces.
- Highest-risk validation: PR-15b live-terminal checks (static harnesses
  previously passed with a dead clock — require a live session assert) and
  PR-10/PR-16 golden re-baselines (review every diff cell, never bulk-accept).

## Risks / Rollback

- Golden churn (PR-10, PR-16, possibly PR-09): keep re-baselines inside their
  PRs so revert = one PR revert. Never bulk-accept golden diffs.
- PR-13 sweep conflicts: keep rebased, land fast, mechanical-only.
- Input bindings (PR-18) may surprise users relying on current keys: bindings
  are additive corrections toward Python; call out behavior changes in the PR
  body.
- Async `remove` (PR-14) changes a public signature: document migration in the
  PR body; keep a sync path if callers exist in-tree.
- Rollback per PR is `git revert` of one merge; branches stay independent
  except PR-12-on-11.

## Open Questions

None — all material facts verified in-tree; the one genuine decision (dispatch
model) is scoped as an RFC, not a question blocking the sequence.
