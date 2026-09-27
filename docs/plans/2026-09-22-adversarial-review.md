# Adversarial parity/logic review — textual-rs (2026-09-22)

Stamp: `922cf93+dirty` (dirty = untracked `docs/plans/` + gitnexus-created
`.claude/`, `AGENTS.md`, `CLAUDE.md` — see §9). Truth checkout:
`/home/shawn/workspace2/textual` (present). Method: 1 orientation/index worker
(gitnexus fresh: 20827 nodes; tilth + ripwire usable) + 7 scoped adversarial
reviewers, all read-only, every claim checked against Python bodies. Final
synthesis step failed on output validation, so this file is the synthesis,
compiled from the 8 landed reports. Reviewer completeness: orientation ✓,
tests ✓, driver ✓; runtime/widgets/events/css/correctness partial (see §8).

## 1. Top risks (fix first)

1. `routing.rs:510` — message pump hard-caps at 1024 msgs and *drops the tail*
   with only a debug log. Python has no cap. Silent message loss under load.
   Remediation: remove cap or spill + warn; add regression test that floods.
2. `option_list.rs:415` — `set_items` **panics** on duplicate ids; sibling
   `add_options` returns `Result`, Python raises catchable `DuplicateID`.
   Remediation: return `Result`, add `DuplicateID`-typed error (§6).
3. `widget_tree.rs:305` — `fire_mount_callbacks` keeps Mount-only, drops
   `Unmount`s for pruned nodes. Python delivers `Unmount` per node. Leaks
   cleanup logic. Remediation: drain both, dispatch per node.
4. `worker.rs:246` — `cancel_by_owner` has zero call sites; unmount purges
   only timers (`event_loop.rs:6062`). Python cancels node workers on unmount.
   Remediation: wire cancel into unmount path; liveness test.
5. `matching.rs:140` — specificity cross-part `u8` sum overflows (`#a #b #c`
   = 300 wraps/panics). Remediation: unbounded tuple compare like Python
   `model.py:210` (also fixes flattened-scalar ordering bug §4).
6. `parser.rs:440` — unknown `:pseudo` widens instead of matching nothing
   (`Button:foobar{}` matches all Buttons). Remediation: unknown → never-match.
7. Harness cannot-fail cluster (§7): `visual_parity_interactive` sole case is
   `pending:true` (never fails); `pty_interactive.rs:1005` leak assert is a
   tautology; `:854` asserts apps MUST differ (goes red when parity lands).
   Remediation: flip pending off, rewrite tautology, rewrite :854 as parity
   assert at fix time. **No parity claim backed only by these is trustworthy.**
8. `screen.rs:575` + `textual_app.rs:1359` — bad/missing CSS path silently
   renders unstyled (no error). Python raises `StylesheetError`. Remediation:
   typed error, fail startup.
9. `theme.rs:482` — `.lock().expect()/unwrap()` everywhere; one poisoned mutex
   panics all later calls (codebase already has poison-tolerant pattern at
   `textual_app.rs:1359` to copy). Remediation: `unwrap_or_else(into_inner)`.
10. Dispatch model gaps (§3): no capture phase exists in Python (Rust invents
    one), no per-message `bubble` flag, no `key_<name>` dispatch, ClickTracker
    has no double/triple-click chain, no mouse capture. These are architectural,
    not one-liners — scope before the closeout plan's P3.

## 2. Runtime / pump / tree / reactive

Purpose: message pump, widget tree attach/detach, lifecycle, reactive fan-out,
workers. Entry points: `src/runtime/routing.rs` (pump+dispatch walk),
`src/widget_tree.rs` (mount/prune/lifecycle), `src/runtime/event_loop.rs`
(reactive phase ~6088, lifecycle drain ~6025, worker phases), `src/worker.rs`.
Live/headless loop order differs (lifecycle↔reactive swapped) — headless-only
bugs will hide from live testing and vice versa.

- BLOCKER pump cap 1024 + drop (§1.1).
- BLOCKER worker cancel dead code (§1.4).
- BUG mount on dead parent inserts orphan + emits Mount (`widget_tree.rs:397`;
  Python gates on `is_attached`).
- BUG unknown-sender fallback broadcasts to all nodes (`routing.rs:609`).
- BUG dynamic watchers fire before node-existence check (`event_loop.rs:6269`).
- GAP coalescer drops non-adjacent same-sender pairs (`routing.rs:435`;
  Python folds head-vs-next only).
- GAP `InputChanged` marked replaceable (`message.rs:112`; Python allows only
  Resize/Update/Layout/UpdateScroll/Prompt).
- GAP no `App.suspend` ctx manager, no suspend/resume signals
  (`runtime/mod.rs:2389`; Python `app.py:4718`).
- GAP `remove` is sync, no `AwaitRemove` (`mod.rs:1807`).
- GAP macros dispatch only `watch_`, only `validate_`, sync fixed-arity
  (`textual-macros/src/reactive.rs:596/432/589`; Python has `_watch_`/`_validate_`,
  0/1/2-arg + async).
- GAP no `bindings`/`toggle_class` flags on reactive set (`reactive.rs:39`).
- FUCKUP `reactive.rs:43` doc contradicts `var()` (`init=true` is correct per
  Python) — fix the doc, not the code.

## 3. Events / dispatch / focus / mouse / App surface

Purpose: event routing, focus chain, key/mouse handling, `App` API.
Entry points: `src/runtime/routing.rs:184` (capture walk), `:594` (bubble),
`src/event/mod.rs`, `src/textual_app.rs`, `src/screen.rs`.

- GAP invented capture phase (root→target) — Python has bubble-only.
- GAP no per-message `bubble` flag (Focus/Blur/ScreenSuspend bubble=False).
- GAP Focus/Blur sent via tree walk; Python posts to one widget; no
  `DescendantFocus/Blur`; `AppFocus(bool)` merges two types; focus flip writes
  state directly instead of posting Blur/Focus pair + scroll + style update.
- GAP focus chain is child-order DFS; Python sorts by `_focus_sort_key`,
  honors `_trap_focus`, skips disabled, inherits visibility. No selector
  filter / maximized clamp on focus move.
- GAP single `on_message` per node, no MRO walk; `prevent_default` never skips
  base handlers. `@on` matches id/class/type only (no `ALLOW_SELECTOR_MATCH`
  kwargs). No `key_<name>` dispatch / `DuplicateKeyHandlers`.
- BUG MouseUp forced to bubble even when capture handled it.
- GAP ClickTracker pairs by identity only — no chained double/triple click;
  no `mouse_captured`/`capture_mouse` redirect.
- GAP `exit()` docstring claims `app.exit()` equivalence; only sets
  `request_stop` (no result/code/message). `app_inline` is a CSS flag, not a
  driver mode. Missing: `open_url`, `deliver_text`, `Binary`, `bell`,
  `export`/`screenshot`; Pilot lacks double/triple-click, mouse down/up,
  `pause(delay)`, `wait_for_animation(s)`, `exit(result)`.
- NIT `runtime/commands.rs` is an internal queue, not `textual.command`
  providers — rename.
- FUCKUP `tests/p1_tree_focus.rs:62` handles Focus/Blur in tree-walk,
  encoding the divergent model — passes iff Focus bubbles, can't falsify it.

## 4. CSS / selectors / cascade / layout

Purpose: stylesheet parse, selector matching, cascade, layout solvers.
Entry points: `src/css/selectors/{parser,matching}.rs`, `resolver.rs`,
`src/layout/`, `src/style.rs:2121`.

- BUG specificity `u8` overflow (§1.5); flattened id=100/class=10 scalar loses
  lexicographic `(id,class,type)` — `#id{red}` vs 10-class `{blue}` picks blue.
- BUG unknown pseudo widens (§1.6); same class covers `:not`/`:nth-child`/typos.
- BUG `:even` inverted (`matching.rs:54`; pinned wrong by test `:292` — fix
  code + test together).
- GAP missing `:enabled/:empty/:first/last-of-type`; invented `:active`
  (Python uses `-active` class).
- GAP cascade `(layer,score,idx)` + keep-earlier-important lets DEFAULT
  `!important` beat user normal; Python key ranks user-normal above
  default-important.
- Layers nearest-wins is real + documented (`KNOWN_GAPS.md:140`) vs Python
  root-wins — keep as intentional divergence, not a work item.
- NIT `A > > B` accepted; Python errors.
- UNINSPECTED: all `src/layout/*` bodies, scrollbar gutter/lane,
  compositor paint/clip/hit-test beyond sort+extent.

## 5. Driver / terminal output

Purpose: crossterm backend, escape sequences, inline mode, render flush.
Entry points: `src/driver/platform/posix.rs`, `src/driver/mod.rs`,
`src/runtime/render.rs`, `src/render/mod.rs:217`.

- GAP always `EnterAlternateScreen`; no inline driver (Python
  `LinuxInlineDriver` never uses alt screen). Blocks closeout P1.
- BUG SYNC-2026 wrapped per-frame, default-on, unnegotiated (Python DECRQM
  query + Apple Terminal exclusion).
- BUG never emits DECSET 2004 yet maps Paste events — live paste arrives raw.
- GAP kitty flags: only DISAMBIGUATE vs 3; `Auto` returns true unconditionally
  (emits unsupported sequences when piped).
- GAP dim blend hardcoded 0.66 always-on RGB-only vs opt-in tunable FILTERS.
- GAP crossterm `Resize` only (no in-band resize); `enable_mouse=false`
  default vs Python `mouse=True`.
- FUCKUP `TEXTUAL_POINTER_SHAPES` override tests dropped; env parsing untested.
- NIT 100ms render tick; diff always emits Home even when clean.

## 6. Correctness cross-cut (errors / locking / API surface)

- BLOCKER `set_items` panics on duplicate ids (§1.2); `get_option` takes
  positional index + returns `Option` vs Python `get_option(option_id)` raising
  `OptionDoesNotExist`; setter renamed `set_items` vs `set_options`.
- BUG silent CSS swallow ×2 (§1.8).
- BUG mutex poisoning (§1.9).
- GAP `src/error.rs` has 4 variants; typed Python failures (StylesheetError,
  DuplicateID, OptionDoesNotExist) collapse to panics or `Error::Message`.
  Remediation: typed variants first, then convert panic sites one by one.
- UNINSPECTED: exhaustive public-API diff beyond sampled OptionList gaps.

## 7. Test harnesses (what the green actually means)

- `visual_parity` 87/87 PASSING==goldens verified, strict on regression —
  trustworthy. `pty_parity` strict-XFail audit clean (both XFails root-caused
  with XPASS-on-fix).
- Cannot-fail cluster (§1.7) — do not cite these as parity evidence.
- `pty_parity` 180/185 cases send zero keys: post-interaction behavior
  unasserted there; interaction coverage lives in `pty_interactive` for a demo
  subset only.
- Glyph-only asserts never fail on colour deltas (input-typing states); ±2/ch
  RGB tolerance is real-bug-sized per visual_parity keystones.
- Stopwatch layout cases skip row 0 (live clock never asserted); `stopwatch06`
  asserts fingerprint-changed, `animation01` colour-progressed, `weather05`
  region-nonempty — settled digits / mid-frames / payloads never exact.
- `interactive_parity.rs` exercises 3 synthetic toy apps, no real demo, no
  Python — proves harness mechanics only.
- `snapshots.rs` guards a constant string — delete or point at real behavior.
- 3 `#[ignore]`s justified (Python reference itself tracebacks on set_reactive01;
  inline01/02 need the missing inline mode; modal ignores covered by passing
  docs_modal cases). Caveat: Rust set_reactive01 example was rewritten with
  `layout:horizontal`, so un-ignoring tests a modified port.
- 2434 `#[test]`s across `src/`; `demo_snapshot.rs`, `driver/platform/*`
  untested (accepted platform gap).

## 8. Unresolved / omitted scope (not verified this pass)

Event-loop bodies partially sampled; `tasks.rs`/`timers.rs`/`pilot.rs`
uninspected; Unmount-loss path not traced to caller. TextArea, Select-overlay,
OptionList (beyond §6), RadioSet, DirectoryTree, MarkdownViewer, Toast, Log,
CommandPalette, containers not body-inspected; per-key Input audit incomplete.
Key alias table vs `keys.py`; palette provider vs `command.py`;
`message_handlers.rs`/`dispatch_ctx` bodies; live-loop vs headless input arms;
ScreenResume/pop-callbacks; MouseMove selection/tooltip. All `src/layout/*`
bodies; compositor paint/clip/hit-test. `apply_size` 0x0 edge; rich-rs escape
bytes. Counterexamples analytic only — nothing was executed (read-only pass);
suite was not run, so "green" is inherited from the handoff, not confirmed.
Critic re-confirmed the top findings (no kills with counter-proof); its
remaining gap list is folded above.

## 9. Collateral + next actions

- `npx gitnexus analyze` (orientation worker) created untracked `.claude/`,
  `AGENTS.md`, `CLAUDE.md` (+ ignored `.gitnexus/`). Harmless; delete if
  unwanted. Pre-existing untracked `docs/plans/` left alone per handoff.
- No source files touched; no commits.
- Suggested remediation order: §1.7 harness honesty first (else fixes can't be
  proven), then §1.1–1.4 + §1.8 (silent wrongness), §1.5–1.6 + §4 selector bugs
  (each needs a ported regression test per house rule), §1.9–1.10 + §5 driver
  flags (P1 inline work will touch this area anyway). §3 dispatch-model items
  need a design decision before code (bubble-only vs capture is architectural).

