#!/usr/bin/env python3
from __future__ import annotations
import argparse
from pathlib import Path

def total_bytes(path: Path) -> int:
    if path.is_file():
        return path.stat().st_size
    return sum(p.stat().st_size for p in path.rglob("*") if p.is_file())

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("path", type=Path)
    ap.add_argument("--max-mb", type=float, required=True)
    args = ap.parse_args()

    size = total_bytes(args.path)
    mb10 = size / 1_000_000
    mib = size / (1024 * 1024)
    print(f"{args.path}: {size:,} bytes = {mb10:.2f} MB = {mib:.2f} MiB")
    if mb10 > args.max_mb:
        raise SystemExit(
            f"SIZE GATE FAILED: {mb10:.2f} MB > {args.max_mb:.2f} MB"
        )
    print("SIZE GATE PASSED")

if __name__ == "__main__":
    main()
