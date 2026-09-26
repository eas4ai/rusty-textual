# rusty-textual: overview

rusty-textual is a Rust port of Python Textual 8.2.8: a framework for
terminal user interfaces with CSS styling, widgets, reactive state, key
bindings and a headless test pilot. It lets a Rust program build a Textual
app through an idiomatic Rust API (typed messages, `#[widget]`,
`ReactiveCtx`) that behaves like the Python original in a real terminal.

It is not a line-by-line transliteration of the Python API, not a widget
set beyond Textual's own, and not a terminal emulator.

Keystone: every user-visible behavior matches Python Textual 8.2.8 unless
the spec records a divergence. The Python source (the sibling `textual`
checkout) is the reference; its tests are not run by this project.

## Spec map

| File | Prefix | Covers |
|---|---|---|
| inline.md | INL | inline render mode |
| terminal.md | TRM | the full-screen terminal behavior inline mode must not change |
| screens.md | SCR | screens pushed on top of the app's own screen |
| updates.md | UPD | an app changing its widgets while it runs |
