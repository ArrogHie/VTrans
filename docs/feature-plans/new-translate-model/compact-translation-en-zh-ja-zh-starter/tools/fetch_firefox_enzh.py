#!/usr/bin/env python3
"""Resolve and optionally download the current Mozilla en->zh Release model.

The registry is the authoritative source. This script avoids pinning an old
firefox-translations-models GitHub path.
"""
from __future__ import annotations
import argparse
import gzip
import hashlib
import json
from pathlib import Path
from urllib.request import urlopen, Request

REGISTRY = "https://storage.googleapis.com/moz-fx-translations-data--303e-prod-translations-data/db/models.json"

def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()

def download(url: str, path: Path) -> None:
    req = Request(url, headers={"User-Agent": "compact-translation-builder/1.0"})
    with urlopen(req) as r, path.open("wb") as out:
        while True:
            chunk = r.read(1024 * 1024)
            if not chunk:
                break
            out.write(chunk)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--output", type=Path, default=Path("models/translation/en-zh"))
    ap.add_argument("--download", action="store_true")
    args = ap.parse_args()

    with urlopen(REGISTRY) as r:
        registry = json.load(r)

    candidates = registry["models"]["en-zh"]
    release = next((x for x in candidates if x.get("releaseStatus") == "Release"), None)
    if release is None:
        raise SystemExit("No en-zh Release model in Mozilla registry")

    base = registry["baseUrl"].rstrip("/")
    files = release["files"]
    selected = {}
    for key in ("model", "srcVocab", "trgVocab", "vocab", "lexicalShortlist"):
        if key in files:
            selected[key] = {
                **files[key],
                "url": f"{base}/{files[key]['path']}"
            }

    print(json.dumps({
        "registryGenerated": registry.get("generated"),
        "architecture": release.get("architecture"),
        "releaseStatus": release.get("releaseStatus"),
        "modelStatistics": release.get("modelStatistics"),
        "metrics": release.get("metrics"),
        "files": selected
    }, ensure_ascii=False, indent=2))

    if not args.download:
        return

    args.output.mkdir(parents=True, exist_ok=True)
    manifest = {
        "registry": REGISTRY,
        "registryGenerated": registry.get("generated"),
        "architecture": release.get("architecture"),
        "releaseStatus": release.get("releaseStatus"),
        "files": {}
    }

    for key, info in selected.items():
        name_gz = Path(info["path"]).name
        gz_path = args.output / name_gz
        print("download", info["url"])
        download(info["url"], gz_path)

        if gz_path.suffix == ".gz":
            out_path = gz_path.with_suffix("")
            with gzip.open(gz_path, "rb") as src, out_path.open("wb") as dst:
                while True:
                    chunk = src.read(1024 * 1024)
                    if not chunk:
                        break
                    dst.write(chunk)
            gz_path.unlink()
        else:
            out_path = gz_path

        manifest["files"][key] = {
            "name": out_path.name,
            "bytes": out_path.stat().st_size,
            "sha256": sha256(out_path)
        }

    (args.output / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2),
        encoding="utf-8"
    )
    print("saved:", args.output)

if __name__ == "__main__":
    main()
