# RFC: Dispatch-model architecture (review §3 structural + §8 gaps)

Status: decided. Each ruling below names the follow-up PR that implements
it (or records an intentional divergence with rationale). No code in this
RFC; spawned PRs land after the remediation sequence.

Grounded in `src/runtime/routing.rs` (capture walk `:184`, bubble `:594`),
`src/runtime/helpers.rs` (`collect_focus_chain_tree`), `src/runtime/mod.rs`
(`set_focus_node`), `src/runtime/event_loop.rs` (Blur/Focus posting
`:4072/4094`, headless mouse arms), `src/message_handlers.rs`
(`MessageHandlers::on`), and Python `screen.py` (`focus_chain`),
`widget.py` (`_focus_sort_key`), `pilot.py`, `app.py`.

## R1 — Capture phase: KEEP as intentional divergence

Python has bubble-only dispatch. Our `on_event_capture` (root→target,
`capabilities.rs:141`) is load-bearing: command palette, content switcher,
help panel, layouts, loading indicator, and markdown viewer all intercept
in capture. Removing it means reworking every interceptor.

Python achieves the same interceptions via `Screen._forward_event`
ordering, not widget API — our capture phase is that ordering formalized
as a trait method. Ruling: keep. Follow-up (audit only, no conversion
mandate): per-site comment citing what Python ordering each capture
implementor replaces.

## R2 — Per-message bubble flags: ADD

Messages always bubble sender→root today (`dispatch_message_bubble`,
stopping on `handled()`); there is no per-type opt-out. Python's
`Message.bubble` defaults True with opt-outs for non-bubbling control
messages. Ruling: add `fn bubble() -> bool` (default `true`) to the
`Message` trait; the bubble loop delivers sender-only when false.
Follow-up PR sets `false` for internal control messages (audit
`message.rs` control types) with per-type tests.

## R3 — Handler walk + `prevent_default`: WIRE, don't widen

Rust has no inheritance, so there is no MRO to walk: the equivalents are
delegate→child forwarding (`delegate.rs`) and the app-level
`MessageHandlers` registry (type-dispatched, registration order). No
change there.

`prevent_default` exists on the envelope (`message.rs:1225`) but the
outcome hardcodes `default_prevented: false` (`routing.rs`) and nothing
reads it. Python semantics: `prevent_default` suppresses the DEFAULT
action, distinct from `stop()` which ends bubbling. Ruling: propagate the
flag into `DispatchOutcome` and define it as "skip the default action",
not "stop bubbling". Follow-up PR audits which handlers perform default
actions and honors the flag, with tests.

## R4 — `@on` selector kwargs: WON'T FIX, document

`MessageHandlers::on` is type-only by design; widget-level handlers take
the concrete message. Python's `@on(..., selector=...)` kwargs have no
equivalent, and adding a selector mini-language to the registry is not
worth it: the established pattern is `query` at the call site.
Ruling: keep type-only dispatch; document the query-at-callsite pattern
in the follow-up PR's docs touch.

## R5 — Focus-chain ordering: SORT + SKIP, then TRAP

Today `collect_focus_chain_tree` is child-order DFS with visibility and
`can_focus_children` handling, but no position sort, no disabled skip,
and no trap scope. Python sorts siblings by `_focus_sort_key`
(`(y, x)` of the virtual region, `widget.py:2378`), scopes the chain to
the nearest `_trap_focus` ancestor (`screen.py:789`), and skips
non-focusable nodes. Ruling: follow-up PR sorts chain siblings by hit-test
rect `(y, x)`, skips disabled nodes, and scopes traversal at trap roots —
in that order of implementation, each with Pilot focus-order tests. The
existing `focus_first_in_active_tree` DFS fallback stays as the empty-chain
degenerate case.

## R6 — Focus/Blur posting: KEEP model, ADD descendant messages

The event loop already posts `Blur`/`Focus` events around transitions
(`event_loop.rs:4072/4094`) — the "tree-walk" characterization is stale
for the transition itself. What's missing: `DescendantFocus`/`DescendantBlur`
notifications to ancestors (used by `:focus-within` styling and containers).
Ruling: keep the posting model; follow-up PR adds the two descendant
messages with tests. (Note: `tests/p1_tree_focus.rs:62` still encodes the
old assumption — its follow-up updates the test alongside the code.)

## R7 — Mouse capture + click chains: EXPOSE + DEFER

`ClickTracker.down_target` already redirects `MouseUp` to the press owner
in both live and headless paths — that IS mouse capture semantically;
there is just no public `capture_mouse` API. Click chains (a single
`Click` carrying a count for double/triple) do not exist: PR-19's
`double_click`/`triple_click` replay plain cycles, documented as such.
Ruling: follow-up PR exposes `App::capture_mouse(node)` /
`release_mouse()` over the existing tracker state, and separately adds
chain counts to `ClickEvent` driven by press timing + position. The two
are independent and may land as separate PRs.

## R8 — `key_<name>` dispatch: ADD as fallback

Python calls `key_<name>` handlers on the focus chain after bindings.
We have bindings only. Ruling: follow-up PR adds a `fn handle_key_name
(&mut self, name: &str, ctx: &mut WidgetCtx) -> bool` hook (default
`false`), invoked for the focused widget when no binding consumes the
key, plus duplicate-handler arbitration documented at the call site.
No `DuplicateKeyHandlers` error type — last-writer-wins with a debug log,
matching our binding-clash posture (`BindingClash`).

## Follow-up PR order

P-A (bubble flags) → P-B (prevent_default wiring) → P-C (focus sort/skip/trap
+ `p1_tree_focus` update) → P-D (descendant focus messages) → P-E
(capture API + click chains) → P-F (`key_<name>` fallback) → P-G (`@on`
docs touch). P-A and P-C are independent and may run in parallel.
