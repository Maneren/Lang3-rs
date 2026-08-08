#!/usr/bin/env python3
"""Aggregate perf samples by function and source line.

Usage:
  perf record -o perf.data -g -F 5000 -- <binary> <args...>
  python3 scripts/perf-profile.py perf.data <binary> [--top N]

Resolves each sampled IP to a source location using llvm-addr2line,
accounting for the module's base offset from the PERF_RECORD_MMAP2 events.
Samples whose IP does not fall inside an executable segment of the target
binary (e.g. kernel addresses) are counted but not resolved.
"""
from __future__ import annotations

import argparse
import subprocess
import sys
from collections import defaultdict


def parse_exec_mmaps(script_out: str) -> list[tuple[int, int, int]]:
    """(start, size, file_offset) triples for r-xp (executable) segments."""
    segs: list[tuple[int, int, int]] = []
    for line in script_out.splitlines():
        if "PERF_RECORD_MMAP2" not in line or ": r-xp " not in line:
            continue
        try:
            after = line.split("PERF_RECORD_MMAP2", 1)[1]
            bracket = after.split("[", 1)[1].split("]", 1)[0]
            start_s, rest = bracket.split("(", 1)
            size_s, off_s = rest.split(") @ ", 1)
            start = int(start_s, 16)
            size = int(size_s, 16)
            off = int(off_s.split(" ", 1)[0], 16)
            segs.append((start, size, off))
        except (ValueError, IndexError):
            continue
    return segs


def extract_ips(script_out: str) -> list[int]:
    """Return the sampled IP (first callchain entry) for every sample line."""
    ips: list[int] = []
    lines = script_out.splitlines()
    for i, line in enumerate(lines):
        if line.startswith("\t") or not line or "PERF_RECORD" in line:
            continue
        if i + 1 >= len(lines) or not lines[i + 1].startswith("\t"):
            continue
        try:
            ips.append(int(lines[i + 1].split()[0], 16))
        except (ValueError, IndexError):
            continue
    return ips


def to_file_offset(ip: int, segs: list[tuple[int, int, int]]) -> int | None:
    for start, size, off in segs:
        if start <= ip < start + size:
            return ip - start + off
    return None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("perf_data", help="path to perf.data")
    ap.add_argument("binary", help="path to the ELF binary (must have debug info)")
    ap.add_argument("--top", type=int, default=20, help="number of entries to print")
    ap.add_argument(
        "--filter",
        default=None,
        help="only show source locations containing this substring (e.g. l3_runtime/src)",
    )
    args = ap.parse_args()

    script_out = subprocess.run(
        ["perf", "script", "-i", args.perf_data, "--show-mmap-events"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    segs = parse_exec_mmaps(script_out)

    offsets = []
    unresolved = 0
    for ip in extract_ips(script_out):
        off = to_file_offset(ip, segs)
        if off is None:
            unresolved += 1
            continue
        offsets.append(off)

    funcs: dict[str, int] = defaultdict(int)
    lines: dict[str, int] = defaultdict(int)
    for i in range(0, len(offsets), 400):
        hexes = [f"0x{o:x}" for o in offsets[i : i + 400]]
        out = subprocess.run(
            ["llvm-addr2line", "-e", args.binary, "-f"] + hexes,
            capture_output=True,
            text=True,
            check=True,
        ).stdout.splitlines()
        for fn, loc in zip(out[0::2], out[1::2]):
            funcs[fn] += 1
            lines[loc if "??" not in loc else "unknown"] += 1

    total = len(offsets)
    print(f"samples: {total} resolved, {unresolved} unresolved (kernel/other)")
    print("\n=== functions (self) ===")
    for name, count in sorted(funcs.items(), key=lambda kv: -kv[1])[: args.top]:
        print(f"{count:6d} {100 * count / total:5.1f}%  {name}")
    print("\n=== source locations (self) ===")
    shown = 0
    for loc, count in sorted(lines.items(), key=lambda kv: -kv[1]):
        if args.filter is not None and args.filter not in loc:
            continue
        print(f"{count:6d} {100 * count / total:5.1f}%  {loc}")
        shown += 1
        if shown >= args.top:
            break
    return 0


if __name__ == "__main__":
    sys.exit(main())
