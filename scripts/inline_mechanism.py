"""Sudus `inline-pty` mechanism: run the inline-mode checks and print one
`sudus: <REQ>: pass|fail` line per requirement.

Test names carry their requirement: `inl_007_...` checks INL-007 and
`trm_001_...` checks TRM-001. The PTY tests live in tests/inline_mode.rs;
library tests named `inl_NNN_...` count too (INL-015's Windows fallback can
only be checked as a unit test on Linux). A requirement passes only when at
least one of its tests ran and none failed, so a missing test or a build
failure fails it.
"""

import re
import subprocess
import sys

REQUIREMENTS = [f"INL-{n:03d}" for n in range(1, 17)] + ["TRM-001"]
RUNS = [
    ["cargo", "test", "--test", "inline_mode", "--", "--test-threads=1"],
    ["cargo", "test", "--lib", "--", "--test-threads=1", "inl_0"],
]
RESULT = re.compile(r"^test (\S+) \.\.\. (ok|FAILED|ignored)", re.MULTILINE)
NAME = re.compile(r"(?:^|::)(inl|trm)_(\d{3})_")


def main() -> int:
    outcomes: dict[str, list[str]] = {req: [] for req in REQUIREMENTS}
    for command in RUNS:
        run = subprocess.run(command, capture_output=True, text=True, stdin=subprocess.DEVNULL)
        output = run.stdout + run.stderr
        print(output)
        for name, status in RESULT.findall(output):
            match = NAME.search(name)
            if match:
                req = f"{match.group(1).upper()}-{match.group(2)}"
                outcomes.setdefault(req, []).append(status)
    failed = False
    for req in REQUIREMENTS:
        statuses = outcomes[req]
        ok = bool(statuses) and all(status == "ok" for status in statuses)
        failed |= not ok
        print(f"sudus: {req}: {'pass' if ok else 'fail'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
