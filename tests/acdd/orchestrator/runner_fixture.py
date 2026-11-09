"""Real child-process behavior for the POSIX role-runner seam tests."""

import json
import os
from pathlib import Path
import signal
import sys
import time


signal.alarm(15)
mode, *arguments = sys.argv[1:]

if mode == "inspect":
    print(json.dumps({
        "argv": arguments,
        "cwd": os.getcwd(),
        "pid": os.getpid(),
        "pgid": os.getpgrp(),
        "forge_session": os.environ.get("FORGE_AGENT_SESSION_ID"),
    }), flush=True)
elif mode == "streams":
    acknowledgment = Path(arguments[0])
    for _ in range(32):
        for descriptor, value in [(1, b"o"), (2, b"e")]:
            remaining = value * 8192
            while remaining:
                remaining = remaining[os.write(descriptor, remaining):]
    deadline = time.monotonic() + 10
    while not acknowledgment.exists():
        if time.monotonic() >= deadline:
            sys.exit(9)
        time.sleep(0.01)
elif mode in ("hang", "orphan"):
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    descendant = os.fork()
    if descendant == 0:
        signal.alarm(15)
        while True:
            signal.pause()
    Path(arguments[0]).write_text(str(descendant))
    if mode == "orphan":
        os._exit(0)
    while True:
        signal.pause()
elif mode == "wait":
    print("ready", flush=True)
    while True:
        signal.pause()
elif mode == "exit":
    sys.exit(7)
elif mode == "binary":
    os.write(1, bytes(range(256)))
    os.write(2, bytes(reversed(range(256))))
elif mode == "signal":
    os.kill(os.getpid(), signal.SIGTERM)
elif mode == "flood":
    while True:
        os.write(1, b"o" * 8192)
        os.write(2, b"e" * 8192)
elif mode == "move":
    os.setpgid(0, os.getpgid(os.getppid()))
    print("moved", flush=True)
    while True:
        signal.pause()
else:
    raise ValueError(f"Unknown fixture mode: {mode}")
