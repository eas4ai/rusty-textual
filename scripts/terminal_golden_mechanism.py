"""Sudus `terminal-golden` mechanism: TRM-002, full-screen frames start from
home, use absolute cursor moves and cover the terminal.

Runs the frame-encoder goldens, the encoder unit test and the PTY check of
a running full-screen app, in the environment from mechanism_env (no
snapshot updates, no force-pass). Passes only when every test named in
EXPECTED ran and passed: a renamed or deleted test fails the check instead
of silently matching nothing.
"""

import re
import subprocess
import sys

from mechanism_env import clean_env

RUNS = [
    [
        "cargo", "test", "--no-fail-fast",
        "--test", "terminal_output_golden",
        "--test", "full_screen_output",
        "--", "--test-threads=1",
    ],
    [
        "cargo", "test", "--lib", "--",
        "--exact", "render::tests::diff_uses_absolute_move_to_for_changed_spans",
    ],
]
EXPECTED = {
    "sparse_frame_update_has_absolute_cursored_deterministic_output",
    "identical_frames_emit_home_only_raw_output",
    "trm_002_full_screen_frames_start_home_and_move_absolutely",
    "render::tests::diff_uses_absolute_move_to_for_changed_spans",
}
RESULT = re.compile(r"^test (\S+) \.\.\. (ok|FAILED|ignored)", re.MULTILINE)


def main() -> int:
    env = clean_env()
    passed: set[str] = set()
    failed = False
    for command in RUNS:
        run = subprocess.run(
            command, capture_output=True, text=True, stdin=subprocess.DEVNULL, env=env
        )
        output = run.stdout + run.stderr
        print(output)
        failed |= run.returncode != 0
        for name, status in RESULT.findall(output):
            if status == "ok":
                passed.add(name)
            else:
                failed = True
    missing = sorted(EXPECTED - passed)
    if missing:
        print(f"terminal-golden: these tests did not run and pass: {', '.join(missing)}")
    return 1 if failed or missing else 0


if __name__ == "__main__":
    sys.exit(main())
