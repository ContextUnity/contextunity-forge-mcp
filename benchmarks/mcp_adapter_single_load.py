#!/usr/bin/env python3
"""Check adapter reads through a warm public MCP request on Linux.

The file access probe observes the actual stdio server. Timing is indicative:
the process and host are shared with other work, so the warm distribution and
TTL-expired call must be compared with a paired baseline.
"""

import argparse
import ctypes
import json
import os
from pathlib import Path
import statistics
import struct
import sys
import time

from mcp_tool_comparison_benchmark import McpClient


IN_OPEN = 0x20
IN_CLOSE_NOWRITE = 0x10


def watch(path: Path) -> int:
    libc = ctypes.CDLL(None, use_errno=True)
    libc.inotify_init1.argtypes = [ctypes.c_int]
    libc.inotify_init1.restype = ctypes.c_int
    libc.inotify_add_watch.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_uint32]
    libc.inotify_add_watch.restype = ctypes.c_int
    fd = libc.inotify_init1(os.O_NONBLOCK | os.O_CLOEXEC)
    if fd < 0:
        raise OSError(ctypes.get_errno(), "inotify_init1 failed")
    if libc.inotify_add_watch(fd, os.fsencode(path), IN_OPEN | IN_CLOSE_NOWRITE) < 0:
        error = OSError(ctypes.get_errno(), "inotify_add_watch failed")
        os.close(fd)
        raise error
    return fd


def drain(fd: int) -> int:
    opened = 0
    while True:
        try:
            data = os.read(fd, 65536)
        except BlockingIOError:
            return opened
        if not data:
            return opened
        offset = 0
        while offset < len(data):
            _, mask, _, name_len = struct.unpack_from("iIII", data, offset)
            opened += bool(mask & IN_OPEN)
            offset += 16 + name_len


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--db", type=Path, required=True)
    parser.add_argument("--expected-opens", type=int, default=1)
    parser.add_argument("--tool", choices=["code_map_inspect", "code_map_search"], default="code_map_search")
    parser.add_argument("--warm-samples", type=int, default=30)
    parser.add_argument("--ttl-wait-seconds", type=float, default=5.1)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    config = args.root / "forge-mcp.yaml"
    if not config.is_file():
        parser.error(f"adapter file missing: {config}")
    if args.output.exists():
        parser.error(f"refusing to overwrite {args.output}")
    if args.warm_samples < 1 or args.ttl_wait_seconds < 5:
        parser.error("warm-samples must be positive and ttl-wait-seconds at least 5")

    tool_args = (
        {"selector": "extensions/commerce/src/contextunity/commerce/modules/matcher/pipeline.py:run_matcher_pipeline", "show_source": False, "show_doc": False}
        if args.tool == "code_map_inspect"
        else {"pattern": "run_matcher_pipeline", "exact": True, "limit": 10}
    )
    fd = watch(config)
    try:
        with McpClient(
            "forge",
            db_path=str(args.db),
            workspace_root=str(args.root),
            binary_path=str(args.binary),
        ) as client:
            first = client.call(args.tool, tool_args)
            if first.error:
                raise RuntimeError(first.error)
            drain(fd)
            second = client.call(args.tool, tool_args)
            opens = drain(fd)
            if second.error:
                raise RuntimeError(second.error)
            warm_ms = [second.wall_ms]
            for _ in range(args.warm_samples - 1):
                result = client.call(args.tool, tool_args)
                if result.error:
                    raise RuntimeError(result.error)
                warm_ms.append(result.wall_ms)
                drain(fd)
            time.sleep(args.ttl_wait_seconds)
            ttl_expired = client.call(args.tool, tool_args)
            if ttl_expired.error:
                raise RuntimeError(ttl_expired.error)
    finally:
        os.close(fd)
    receipt = {
        "binary": str(args.binary),
        "root": str(args.root),
        "db": str(args.db),
        "tool": args.tool,
        "adapter_open_events_for_one_warm_call": opens,
        "expected": args.expected_opens,
        "first_ms": first.wall_ms,
        "warm_samples": len(warm_ms),
        "warm_median_ms": statistics.median(warm_ms),
        "warm_p95_ms": sorted(warm_ms)[int(0.95 * (len(warm_ms) - 1))],
        "ttl_expired_ms": ttl_expired.wall_ms,
    }
    args.output.write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt))
    return 0 if opens == args.expected_opens else 1


if __name__ == "__main__":
    sys.exit(main())
