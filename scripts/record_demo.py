"""Scripted demo recording: drive the showcase examples in a PTY and write one
asciicast v2 file. Ported from reactive-tui's scripts/record-demo.py.

Usage:
    python3 scripts/record_demo.py /tmp/examples.cast
    agg --font-family "DejaVu Sans Mono" /tmp/examples.cast /tmp/examples.gif

The font flag matters: agg's default font renders U+2800 blank Braille as
dotted tofu, which looks like a framework bug and is not one.

Segments without keys just boot, settle, and get killed on timeout: enough
for a layout look. Apps without a quit binding are terminated via timeout.
"""
import json
import os
import pty
import select
import subprocess
import sys
import termios
import time
import fcntl
import struct

WIDTH, HEIGHT = 100, 30

# (example name, [(delay_before_send, bytes_to_send), ...], settle_after_last)
# First delay in each segment is generous: it covers cargo startup.
PLAN = [
    ("calculator",
     [(6.0, b"7"), (1.5, b"*"), (1.5, b"8"), (1.5, b"=")],
     1.5),
    ("reminder",
     [(6.0, b"Buy milk"), (1.5, b"\t")],
     1.5),
    ("merlin",
     [(6.0, b"1"), (2.0, b"5")],
     1.5),
    ("dictionary",
     [(6.0, b"rust"), (1.5, b"\r")],
     1.5),
    ("five_by_five", [], 2.0),
    ("code_browser", [], 2.0),
    ("diff", [], 2.0),
    ("json_tree", [], 2.0),
    ("markdown", [], 2.0),
]

events = []
t0 = time.monotonic()


def stamp():
    return round(time.monotonic() - t0, 6)


def drain(master, deadline):
    while time.monotonic() < deadline:
        r, _, _ = select.select([master], [], [], 0.05)
        if not r:
            continue
        try:
            chunk = os.read(master, 65536)
        except OSError:
            chunk = b""
        if chunk:
            events.append([stamp(), "o",
                           chunk.decode("utf-8", errors="replace")])
        else:
            return False
    return True


def run_segment(example, script, settle):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", HEIGHT, WIDTH, 0, 0))
    env = dict(os.environ, TERM="xterm-256color",
               COLUMNS=str(WIDTH), LINES=str(HEIGHT))
    proc = subprocess.Popen(
        ["cargo", "run", "-q", "--example", example],
        stdin=slave, stdout=slave, stderr=slave,
        env=env, close_fds=True,
    )
    os.close(slave)
    os.set_blocking(master, False)
    alive = True
    for delay, keys in script:
        if alive:
            alive = drain(master, time.monotonic() + delay)
        if not alive or proc.poll() is not None:
            alive = False
            break
        try:
            os.write(master, keys)
        except OSError:
            alive = False
            break
    if alive:
        drain(master, time.monotonic() + settle)
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
    os.close(master)
    print(f"{example}: exit={proc.returncode} events={len(events)}", flush=True)


for example, script, settle in PLAN:
    run_segment(example, script, settle)
    time.sleep(0.5)

out = sys.argv[1] if len(sys.argv) > 1 else "demo.cast"
with open(out, "w") as f:
    f.write(json.dumps({"version": 2, "width": WIDTH, "height": HEIGHT,
                        "timestamp": int(time.time()),
                        "env": {"TERM": "xterm-256color", "SHELL": "/bin/bash"}}) + "\n")
    for ev in events:
        f.write(json.dumps(ev) + "\n")
print(f"wrote {out}: {len(events)} events, "
      f"{os.path.getsize(out)/1e6:.1f} MB", flush=True)
