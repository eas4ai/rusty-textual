"""Sudus `strict-clippy` mechanism: check BLD-001 and print one
`sudus: BLD-001: pass|fail` line.

Each place the zero-warning bar covers (the root crate, textual-macros, the
docs/examples workspace and the inline probe) must build with the pinned
toolchain, and strict clippy (`cargo clippy --all-targets -- -W
clippy::pedantic`, never `-D warnings`) must report no warning there. Every
distinct warning and error is printed; a place whose cargo run fails, or
whose `rustc --version` is not the pinned one, fails the requirement too.
Cargo runs in the environment from mechanism_env, which drops the
environment variables that would swap the toolchain, its flags or clippy's
configuration; cargo config files still apply.
"""

import json
import re
import subprocess
import sys

# No __pycache__ next to the scripts: Sudus counts it as an undeclared change.
sys.dont_write_bytecode = True

from mechanism_env import clean_env  # noqa: E402

TOOLCHAIN = "rustc 1.98.0 "
STRICT = ["--all-targets", "--message-format=json", "--", "-W", "clippy::pedantic"]
# (name, working directory, cargo arguments before the strict ones)
PLACES = [
    ("root crate", ".", []),
    ("textual-macros", "textual-macros", []),
    ("docs/examples", "docs/examples", ["--workspace"]),
    (
        "inline probe",
        ".",
        [
            "--manifest-path",
            "tests/fixtures/inline_probe/Cargo.toml",
            "--target-dir",
            "target/inline-probe",
        ],
    ),
]
# rustc's closing count ("5 warnings emitted") is not a warning of its own.
SUMMARY = re.compile(r"^\d+ warnings? emitted")


def diagnostics(stdout: str) -> list[str]:
    """The distinct warnings and errors in cargo's JSON output, one line each."""
    found: list[str] = []
    for line in stdout.splitlines():
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get("reason") != "compiler-message":
            continue
        diagnostic = message["message"]
        level = diagnostic["level"]
        if level not in ("warning", "error") or SUMMARY.match(diagnostic["message"]):
            continue
        spans = [span for span in diagnostic["spans"] if span.get("is_primary")]
        where = f"{spans[0]['file_name']}:{spans[0]['line_start']}" if spans else "-"
        code = (diagnostic.get("code") or {}).get("code", "-")
        entry = f"{level} {code} {where}: {diagnostic['message']}"
        if entry not in found:
            found.append(entry)
    return found


def main() -> int:
    env = clean_env()
    failed = False
    for name, cwd, args in PLACES:
        version = subprocess.run(
            ["rustc", "--version"], cwd=cwd, capture_output=True, text=True, env=env
        ).stdout.strip()
        if not version.startswith(TOOLCHAIN):
            print(f"{name}: toolchain {version!r}, want {TOOLCHAIN.strip()!r}")
            failed = True
        run = subprocess.run(
            ["cargo", "clippy", *args, *STRICT],
            cwd=cwd,
            capture_output=True,
            text=True,
            stdin=subprocess.DEVNULL,
            env=env,
        )
        found = diagnostics(run.stdout)
        for entry in found:
            print(f"{name}: {entry}")
        if run.returncode != 0:
            print(f"{name}: cargo clippy exited {run.returncode}")
            print(run.stderr[-4000:])
        print(f"{name}: {version}, {len(found)} warnings or errors")
        failed |= bool(found) or run.returncode != 0
    print(f"sudus: BLD-001: {'fail' if failed else 'pass'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
