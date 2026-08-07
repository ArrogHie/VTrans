"""Backfill the v6 model manifest with real SHA-256 / size_bytes values.

Computes hashes from the actual conversion artifacts, deploys them into the
models directory, and writes the values into `manifest.json` (both the
src-tauri copy and the crate template). Fails if any expected file is
missing. Never writes placeholder hashes.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
from pathlib import Path


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--det", required=True, type=Path)
    parser.add_argument("--rec", required=True, type=Path)
    parser.add_argument("--dict", required=True, type=Path)
    parser.add_argument("--dict-name", required=True)
    parser.add_argument("--deploy-dir", required=True, type=Path)
    args = parser.parse_args()

    for p in (args.det, args.rec, args.dict):
        if not p.exists():
            print(f"ERROR: artifact not found: {p}", file=sys.stderr)
            return 1

    det_sha = sha256_of(args.det)
    rec_sha = sha256_of(args.rec)
    dict_sha = sha256_of(args.dict)
    det_size = args.det.stat().st_size
    rec_size = args.rec.stat().st_size

    # Deploy artifacts into the models directory. Only one rec copy is
    # deployed: rec_ja / rec_en / rec_multi all point at `rec.onnx`.
    args.deploy_dir.mkdir(parents=True, exist_ok=True)
    targets = {
        "det.onnx": args.det,
        "rec.onnx": args.rec,
        args.dict_name: args.dict,
    }
    for name, src in targets.items():
        dest = args.deploy_dir / name
        if dest.exists() and dest.samefile(src):
            print(f"  skip   {name} (already in place)")
            continue
        shutil.copyfile(src, dest)
        print(f"  deploy {name}")

    # Idempotent convergence: remove stale multi-copy rec files left by the
    # previous layout so repeated runs always end in a single-rec state.
    stale_rec = ["rec_en.onnx", "rec_multi.onnx"]
    for name in stale_rec:
        stale = args.deploy_dir / name
        if stale.exists():
            stale.unlink()
            print(f"  remove {name} (stale rec copy)")

    # Update the manifest JSON.
    manifest_path = args.manifest
    if not manifest_path.exists():
        print(f"ERROR: manifest not found: {manifest_path}", file=sys.stderr)
        return 1
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))

    ocr = manifest["ocr"]
    ocr["det"].update(id="ppocr-det-v6", sha256=det_sha, size_bytes=det_size)
    for key in ("rec_ja", "rec_en", "rec_multi"):
        ocr[key].update(
            id={
                "rec_ja": "ppocr-rec-v6",
                "rec_en": "ppocr-rec-v6-en",
                "rec_multi": "ppocr-rec-v6-multi",
            }[key],
            path="ocr/rec.onnx",
            sha256=rec_sha,
            size_bytes=rec_size,
        )
    ocr["dicts"] = {
        "ja": f"ocr/{args.dict_name}",
        "en": f"ocr/{args.dict_name}",
        "auto": f"ocr/{args.dict_name}",
    }

    # Keep the original compact style: arrays on one line, objects indented.
    def compact(obj):
        return json.dumps(obj, ensure_ascii=False, separators=(",", ": "))

    def render(node, indent: int) -> str:
        pad = "  " * indent
        if isinstance(node, dict):
            if not node:
                return "{}"
            lines = ["{"]
            items = list(node.items())
            for i, (k, v) in enumerate(items):
                comma = "," if i < len(items) - 1 else ""
                if isinstance(v, (dict, list)):
                    lines.append(f"{pad}  {compact(k)}: {render(v, indent + 1)}{comma}")
                else:
                    lines.append(f"{pad}  {compact(k)}: {compact(v)}{comma}")
            lines.append(f"{pad}}}")
            return "\n".join(lines)
        if isinstance(node, list):
            if not node:
                return "[]"
            if all(not isinstance(x, (dict, list)) for x in node):
                return compact(node)
            lines = ["["]
            for i, v in enumerate(node):
                comma = "," if i < len(node) - 1 else ""
                lines.append(f"{pad}  {render(v, indent + 1)}{comma}")
            lines.append(f"{pad}]")
            return "\n".join(lines)
        return compact(node)

    manifest_path.write_text(
        render(manifest, 0) + "\n",
        encoding="utf-8",
    )

    print(f"backfilled {manifest_path}")
    print(f"  det  sha256={det_sha} size={det_size}")
    print(f"  rec  sha256={rec_sha} size={rec_size}")
    print(f"  dict sha256={dict_sha} lines={sum(1 for _ in args.dict.open('r', encoding='utf-8'))}")
    print(f"  deployed to {args.deploy_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
