"""The environment the Sudus mechanism wrappers run cargo in.

The checks must give the same answer whatever the caller's shell holds, so
the variables that change what the tests check, or where cargo puts the
binaries they run, are removed or pinned:

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
PINNED = {"COLORTERM": "truecolor", "INSTA_UPDATE": "no", "INSTA_FORCE_PASS": "0"}


def clean_env() -> dict[str, str]:
    """A copy of this process's environment, cleaned as described above."""
    env = {
        key: value
        for key, value in os.environ.items()
        if key not in REMOVED and not key.startswith(REMOVED_PREFIXES)
    }
    env.update(PINNED)
    return env
