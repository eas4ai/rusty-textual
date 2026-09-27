"""Sudus `update-pty` mechanism: run the widget-update checks and print one
`sudus: <REQ>: pass|fail` line per requirement.

Test names carry their requirement: `upd_001_...` checks UPD-001. The PTY
tests live in tests/widget_updates.rs. A requirement passes only when at
least one of its tests ran and none failed, so a missing test or a build
failure fails it. Cargo runs in the environment from mechanism_env; the PTY
tests also start each app with a cleared environment.
"""

import re
import subprocess
import sys

# No __pycache__ next to the scripts: Sudus counts it as an undeclared change.
sys.dont_write_bytecode = True

from mechanism_env import clean_env  # noqa: E402

REQUIREMENTS = ["UPD-001", "UPD-002"]
COMMAND = ["cargo", "test", "--test", "widget_updates", "--", "--test-threads=1"]
RESULT = re.compile(r"^test (\S+) \.\.\. (ok|FAILED|ignored)", re.MULTILINE)
NAME = re.compile(r"(?:^|::)upd_(\d{3})_")


def main() -> int:
    outcomes: dict[str, list[str]] = {req: [] for req in REQUIREMENTS}
    run = subprocess.run(
        COMMAND, capture_output=True, text=True, stdin=subprocess.DEVNULL, env=clean_env()
    )
    output = run.stdout + run.stderr
    print(output)
    for name, status in RESULT.findall(output):
        match = NAME.search(name)
        if match:
            outcomes.setdefault(f"UPD-{match.group(1)}", []).append(status)
    failed = False
    for req in REQUIREMENTS:
        statuses = outcomes[req]
        ok = bool(statuses) and all(status == "ok" for status in statuses)
        failed |= not ok
        print(f"sudus: {req}: {'pass' if ok else 'fail'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
