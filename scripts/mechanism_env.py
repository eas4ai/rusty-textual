"""The environment the Sudus mechanism wrappers run cargo in.

The checks must give the same answer whatever the caller's shell holds, so
the variables that change what the tests check, or where cargo puts the
binaries they run, are removed or pinned:

- `RUSTUP_TOOLCHAIN`, `RUSTC`, `RUSTC_WRAPPER`, `RUSTFLAGS`,
  `CARGO_ENCODED_RUSTFLAGS`, cargo's environment forms of the same settings
  (`CARGO_BUILD_RUSTC`, `CARGO_BUILD_RUSTC_WRAPPER`,
  `CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER`, `CARGO_BUILD_RUSTFLAGS`,
  `CARGO_TARGET_<triple>_RUSTFLAGS`) and `CLIPPY_CONF_DIR`: swap the
  toolchain the repository pins (BLD-001), the flags it compiles with or
  clippy's configuration, which can hide warnings. Cargo config files
  still apply; this checkout's `.cargo/config.toml` sets only its target
  directory.
- `CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET_DIR`: the parity tests run docs
  example binaries from docs/examples/target; a redirected build would
  leave stale ones there. (The docs build also passes `--target-dir`, which
  beats a `build.target-dir` in a cargo config file.)
- `REPORT_ONLY`: turns off visual_parity's regression assertion.
- `INSTA_*`: can force-pass or rewrite snapshots. `INSTA_UPDATE=no` and
  `INSTA_FORCE_PASS=0` make a mismatch fail without writing anything; the
  environment also overrides an insta.yaml config file that asks otherwise.
- `TEXTUAL_*`, `NO_COLOR`, `FORCE_COLOR`, `CLICOLOR_FORCE`, `COLUMNS`,
  `LINES`: change what an app draws; PTY children that inherit them fail
  or pass for the wrong reason.
- `DEBUG_CASE`, `DUMP_CASE`, `DUMP_FILE`: parity-test debugging switches.
- `COLORTERM` is pinned to `truecolor`, the value the goldens were
  checked under.
"""

import os

REMOVED = {
    "RUSTUP_TOOLCHAIN",
    "RUSTC",
    "RUSTC_WRAPPER",
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_RUSTC",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTFLAGS",
    "CLIPPY_CONF_DIR",
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_TARGET_DIR",
    "REPORT_ONLY",
    "NO_COLOR",
    "FORCE_COLOR",
    "CLICOLOR_FORCE",
    "COLUMNS",
    "LINES",
    "DEBUG_CASE",
    "DUMP_CASE",
    "DUMP_FILE",
}
REMOVED_PREFIXES = ("TEXTUAL_", "INSTA_")


def target_rustflags(key: str) -> bool:
    """Whether `key` is cargo's `CARGO_TARGET_<triple>_RUSTFLAGS`."""
    return key.startswith("CARGO_TARGET_") and key.endswith("_RUSTFLAGS")
PINNED = {"COLORTERM": "truecolor", "INSTA_UPDATE": "no", "INSTA_FORCE_PASS": "0"}


def clean_env() -> dict[str, str]:
    """A copy of this process's environment, cleaned as described above."""
    env = {
        key: value
        for key, value in os.environ.items()
        if key not in REMOVED
        and not key.startswith(REMOVED_PREFIXES)
        and not target_rustflags(key)
    }
    env.update(PINNED)
    return env
