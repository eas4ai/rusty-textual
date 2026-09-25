"""Sudus `pty-parity` mechanism: TRM-003, full-screen apps keep matching the
Python goldens.

Runs pty_parity, visual_parity and visual_parity_interactive in the
environment from mechanism_env: `REPORT_ONLY` cannot turn off the visual
regression assertion, and without `CARGO_TARGET_DIR` the docs examples that
pty_parity builds land in docs/examples/target, where the tests run them.

Passes only when every test binary ran and passed exactly the pinned number
of tests, so a deleted or renamed case fails the check. Update EXPECTED
when cases are added.
"""

import re
import subprocess
import sys

# No __pycache__ next to the scripts: Sudus counts it as an undeclared change.
sys.dont_write_bytecode = True

from mechanism_env import clean_env  # noqa: E402

COMMAND = [
    "cargo", "test", "--no-fail-fast",
    "--test", "pty_parity",
    "--test", "visual_parity",
    "--test", "visual_parity_interactive",
    "--", "--test-threads=1",
]
EXPECTED = {"pty_parity": 185, "visual_parity": 1, "visual_parity_interactive": 1}
RUNNING = re.compile(r"^\s*Running tests/(\w+)\.rs", re.MULTILINE)
RESULT = re.compile(r"^test result: (\w+)\. (\d+) passed; (\d+) failed", re.MULTILINE)


def main() -> int:
    # One stream, so each "Running" line (stderr) stays before its result
    # line (stdout).
    run = subprocess.run(
        COMMAND,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        stdin=subprocess.DEVNULL,
        env=clean_env(),
    )
    output = run.stdout
    print(output)
    # Pair each "Running tests/<name>.rs" line with the result line in its
    # own section; a binary that crashed has none.
    passed: dict[str, int] = {}
    sections = list(RUNNING.finditer(output))
    for index, running in enumerate(sections):
        end = sections[index + 1].start() if index + 1 < len(sections) else len(output)
        result = RESULT.search(output, running.end(), end)
        if result and result.group(1) == "ok" and result.group(3) == "0":
            passed[running.group(1)] = int(result.group(2))
    wrong = {
        name: passed.get(name)
        for name, count in EXPECTED.items()
        if passed.get(name) != count
    }
    for name, got in wrong.items():
        print(f"pty-parity: {name} expected {EXPECTED[name]} passing tests, got {got}")
    return 1 if run.returncode != 0 or wrong else 0


if __name__ == "__main__":
    sys.exit(main())
