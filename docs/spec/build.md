Prefix: BLD

# Build and warnings

The toolchain the port builds with and the warnings bar its code meets.
"Strict clippy" means `cargo clippy --all-targets -- -W clippy::pedantic`.

[BLD-001] The repository MUST pin the Rust toolchain it builds with to
1.98.0, with clippy and rustfmt, and strict clippy MUST report no warnings
with that toolchain in the root crate, the `textual-macros` crate, the
`docs/examples` workspace and the inline probe.
Falsifier: In the root crate, textual-macros, the docs/examples workspace or tests/fixtures/inline_probe, the toolchain cargo uses is not 1.98.0, or strict clippy reports a warning.
Mechanism: strict-clippy
Rationale: The developer's zero-warning bar; the pin is per project and leaves the machine's default toolchain alone (developer, 2026-09-27), and the Windows host's strict-clippy pass is builder evidence in the commitment's done-when, since a check runs on local inputs.
Status: Agreed 2026-09-27
