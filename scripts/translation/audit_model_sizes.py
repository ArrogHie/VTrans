#!/usr/bin/env python3
"""Audit translation model directory sizes against the v2 manifest budget.

Two modes:

1. Per-pair audit of the whole `translation/` directory (the CI gate):
   en-zh <= 65 MB, ja-zh <= 110 MB, total <= 200 MB (hard) / 175 MB
   (target). The budgets are read from the manifest
   (`translation.budget_mb`) when `--manifest` is given, otherwise the
   v2 defaults are used; explicit flags override both.

2. Single-directory gate (`--max-mb`), used by the ja-zh conversion
   script to check one pair immediately after conversion.

Sizes are measured in decimal MB (1 MB = 1_000_000 bytes), matching the
budget constants in the manifest. Per-pair download manifests
(`manifest.json` next to the model files) are excluded from the count.

`--self-test` validates the gate itself against synthetic directories:
it returns 0 when both a within-budget and an over-budget directory are
classified correctly, and 1 otherwise (usable in CI).

Exit codes:
  0  gate passed (or self-test passed)
  1  gate failed (over budget, missing dir, or self-test failure)
  2  usage error
"""

from __future__ import annotations

import argparse
import json
import sys
import tempfile
from pathlib import Path

DEFAULT_HARD_MB = 200
DEFAULT_TARGET_MB = 175
DEFAULT_EN_ZH_MB = 65
DEFAULT_JA_ZH_MB = 110

# Per-pair download manifests are build artifacts, not shipped model files.
EXCLUDED_NAMES = {"manifest.json"}


def dir_bytes(path: Path) -> int:
    if path.is_file():
        return path.stat().st_size
    total = 0
    for p in path.rglob("*"):
        if p.is_file() and p.name not in EXCLUDED_NAMES:
            total += p.stat().st_size
    return total


def fmt_mb(size: int) -> float:
    return size / 1_000_000


def audit_pair(path: Path, budget_mb: float) -> tuple[int, bool]:
    size = dir_bytes(path)
    mb = fmt_mb(size)
    ok = mb <= budget_mb
    print(f"  {path.name}: {size:,} bytes = {mb:.2f} MB "
          f"(budget {budget_mb:.2f} MB) {'OK' if ok else 'OVER'}")
    return size, ok


def run_audit(
    translation_dir: Path,
    manifest: Path | None,
    hard_mb: float | None,
    target_mb: float | None,
    en_zh_mb: float | None,
    ja_zh_mb: float | None,
) -> int:
    if not translation_dir.is_dir():
        print(f"ERROR: translation directory not found: {translation_dir}", file=sys.stderr)
        return 1

    if manifest is not None:
        data = json.loads(manifest.read_text(encoding="utf-8"))
        budget = (data.get("translation") or {}).get("budget_mb", {})
        hard_mb = hard_mb if hard_mb is not None else budget.get("hard_mb", DEFAULT_HARD_MB)
        target_mb = target_mb if target_mb is not None else budget.get("target_mb", DEFAULT_TARGET_MB)
        en_zh_mb = en_zh_mb if en_zh_mb is not None else budget.get("en_zh_mb", DEFAULT_EN_ZH_MB)
        ja_zh_mb = ja_zh_mb if ja_zh_mb is not None else budget.get("ja_zh_mb", DEFAULT_JA_ZH_MB)
    else:
        hard_mb = hard_mb if hard_mb is not None else DEFAULT_HARD_MB
        target_mb = target_mb if target_mb is not None else DEFAULT_TARGET_MB
        en_zh_mb = en_zh_mb if en_zh_mb is not None else DEFAULT_EN_ZH_MB
        ja_zh_mb = ja_zh_mb if ja_zh_mb is not None else DEFAULT_JA_ZH_MB

    print(f"audit: {translation_dir}")
    en_zh_dir = translation_dir / "en-zh"
    ja_zh_dir = translation_dir / "ja-zh"
    en_zh_size, en_zh_ok = audit_pair(en_zh_dir, en_zh_mb)
    ja_zh_size, ja_zh_ok = audit_pair(ja_zh_dir, ja_zh_mb)

    total = en_zh_size + ja_zh_size
    total_mb = fmt_mb(total)
    print(f"  total: {total:,} bytes = {total_mb:.2f} MB "
          f"(target {target_mb:.2f} MB / hard {hard_mb:.2f} MB)")

    over = []
    if not en_zh_ok:
        over.append(f"en-zh {fmt_mb(en_zh_size):.2f} MB > {en_zh_mb:.2f} MB")
    if not ja_zh_ok:
        over.append(f"ja-zh {fmt_mb(ja_zh_size):.2f} MB > {ja_zh_mb:.2f} MB")
    if total_mb > hard_mb:
        over.append(f"total {total_mb:.2f} MB > {hard_mb:.2f} MB (hard)")
    if total_mb > target_mb:
        print(f"  WARN: total exceeds target budget ({target_mb:.2f} MB) "
              f"but is within the hard limit")

    if over:
        print("SIZE GATE FAILED:")
        for item in over:
            print(f"  - {item}")
        return 1
    print("SIZE GATE PASSED")
    return 0


def run_single(path: Path, max_mb: float) -> int:
    if not path.exists():
        print(f"ERROR: path not found: {path}", file=sys.stderr)
        return 1
    size = dir_bytes(path)
    mb = fmt_mb(size)
    print(f"{path}: {size:,} bytes = {mb:.2f} MB = "
          f"{size / (1024 * 1024):.2f} MiB")
    if mb > max_mb:
        print(f"SIZE GATE FAILED: {mb:.2f} MB > {max_mb:.2f} MB")
        return 1
    print(f"SIZE GATE PASSED (<= {max_mb:.2f} MB)")
    return 0


def self_test() -> int:
    """Verify the gate classifies synthetic dirs correctly."""
    failures = 0
    with tempfile.TemporaryDirectory(prefix="vtrans-audit-selftest-") as tmp:
        root = Path(tmp)

        # Within budget: tiny files, generous budgets.
        pass_dir = root / "pass"
        (pass_dir / "en-zh").mkdir(parents=True)
        (pass_dir / "ja-zh").mkdir(parents=True)
        (pass_dir / "en-zh" / "model.bin").write_bytes(b"x" * 1024)
        (pass_dir / "ja-zh" / "model.bin").write_bytes(b"y" * 1024)
        rc = run_audit(pass_dir, None, 1.0, 1.0, 1.0, 1.0)
        if rc != 0:
            print("SELF-TEST FAILED: within-budget dir was rejected", file=sys.stderr)
            failures += 1
        else:
            print("SELF-TEST OK: within-budget dir passed")

        # Over budget: same tiny files, sub-megabyte budgets.
        over_dir = root / "over"
        (over_dir / "en-zh").mkdir(parents=True)
        (over_dir / "ja-zh").mkdir(parents=True)
        (over_dir / "en-zh" / "model.bin").write_bytes(b"x" * 1024)
        (over_dir / "ja-zh" / "model.bin").write_bytes(b"y" * 1024)
        rc = run_audit(over_dir, None, 0.001, 0.001, 0.001, 0.001)
        if rc == 0:
            print("SELF-TEST FAILED: over-budget dir was accepted", file=sys.stderr)
            failures += 1
        else:
            print("SELF-TEST OK: over-budget dir rejected")

    if failures:
        print("SELF-TEST FAILED", file=sys.stderr)
        return 1
    print("SELF-TEST PASSED")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "path",
        nargs="?",
        type=Path,
        help="translation directory (en-zh/ + ja-zh/) or a single directory",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        help="v2 manifest.json; budgets are read from translation.budget_mb",
    )
    parser.add_argument("--hard-mb", type=float, help="total hard budget in MB")
    parser.add_argument("--target-mb", type=float, help="total target budget in MB")
    parser.add_argument("--en-zh-mb", type=float, help="en-zh budget in MB")
    parser.add_argument("--ja-zh-mb", type=float, help="ja-zh budget in MB")
    parser.add_argument(
        "--max-mb",
        type=float,
        help="single-directory gate (overrides per-pair mode)",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="validate the gate against synthetic directories and exit",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()
    if args.path is None:
        parser.print_usage(sys.stderr)
        return 2
    if args.max_mb is not None:
        return run_single(args.path, args.max_mb)
    return run_audit(
        args.path,
        args.manifest,
        args.hard_mb,
        args.target_mb,
        args.en_zh_mb,
        args.ja_zh_mb,
    )


if __name__ == "__main__":
    sys.exit(main())
