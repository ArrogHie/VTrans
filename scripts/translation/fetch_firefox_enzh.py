#!/usr/bin/env python3
"""Fetch the pinned Mozilla Firefox en->zh Release model (Bergamot).

Resolves the current en-zh `base-memory` Release entry from the Mozilla
model registry, downloads the four model files (model, source/target
SentencePiece vocabularies, lexical shortlist), verifies their SHA-256,
and writes a per-pair download manifest next to the artifacts.

The registry is the single source of truth for the en-zh model. The
selected entry is pinned by its registry `generated` timestamp and model
revision directory, which are recorded in the download manifest and later
carried into `src-tauri/resources/models/manifest.json` metadata.

The model binary and vocabularies are NOT committed to Git
(`src-tauri/resources/models/translation/*` is ignored).

Exit codes:
  0  success
  1  download or verification failure
  2  usage error
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import sys
from pathlib import Path
from urllib.request import Request, urlopen

# Pinned registry endpoint (B4: freeze revision; never follow "latest"
# silently — the resolved entry is recorded and must be reviewed on bump).
REGISTRY_URL = (
    "https://storage.googleapis.com/moz-fx-translations-data--303e-prod-"
    "translations-data/db/models.json"
)

# The four Bergamot files shipped for en-zh. Keys match the registry
# `files` keys; local names are the decompressed file names.
REQUIRED_FILES = ("model", "srcVocab", "trgVocab", "lexicalShortlist")

UA = "vtrans-models-builder/1.0"


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def fetch_bytes(url: str) -> bytes:
    req = Request(url, headers={"User-Agent": UA})
    with urlopen(req, timeout=60) as resp:
        return resp.read()


def download(url: str, dest: Path) -> None:
    req = Request(url, headers={"User-Agent": UA})
    with urlopen(req, timeout=120) as resp, dest.open("wb") as out:
        while True:
            chunk = resp.read(1024 * 1024)
            if not chunk:
                break
            out.write(chunk)


def gunzip_to(src: Path, dest: Path) -> None:
    with gzip.open(src, "rb") as gz, dest.open("wb") as out:
        while True:
            chunk = gz.read(1024 * 1024)
            if not chunk:
                break
            out.write(chunk)


def select_release(registry: dict) -> dict:
    """Pick the en-zh Release entry, preferring `base-memory`.

    The registry may contain several en-zh entries (e.g. `base` dev builds
    with `releaseStatus: null`). We require `releaseStatus == "Release"`
    and prefer `architecture == "base-memory"` (the recommended footprint).
    """
    candidates = registry.get("models", {}).get("en-zh", [])
    releases = [e for e in candidates if e.get("releaseStatus") == "Release"]
    if not releases:
        raise RuntimeError("no en-zh Release entry found in the Mozilla registry")
    for entry in releases:
        if entry.get("architecture") == "base-memory":
            return entry
    # No base-memory entry: fall back to the first Release (registry drift
    # is visible in the download manifest for review).
    print(
        f"WARN: no base-memory Release entry; using "
        f"{releases[0].get('architecture')!r}",
        file=sys.stderr,
    )
    return releases[0]


def build_plan(registry: dict, release: dict) -> tuple[dict, dict]:
    """Return (plan, resolved_files) for the selected release."""
    base_url = registry["baseUrl"].rstrip("/")
    files = release["files"]
    resolved: dict[str, dict] = {}
    for key in REQUIRED_FILES:
        info = files[key]
        resolved[key] = {
            **info,
            "url": f"{base_url}/{info['path']}",
        }
    plan = {
        "registry": REGISTRY_URL,
        "registryGenerated": registry.get("generated"),
        "architecture": release.get("architecture"),
        "releaseStatus": release.get("releaseStatus"),
        "sourceLanguage": release.get("sourceLanguage"),
        "targetLanguage": release.get("targetLanguage"),
        # Path layout: models/<pair>/<revision>/exported/<file>.gz
        "revision": Path(files["model"]["path"]).parent.parent.name,
        "modelStatistics": release.get("modelStatistics"),
        "metrics": release.get("metrics"),
        "files": {k: v for k, v in resolved.items()},
    }
    return plan, resolved


def verify_and_record(output: Path, resolved: dict) -> dict:
    """Download, decompress, verify and record every required file.

    The registry publishes `uncompressedHash` / `uncompressedSize` for the
    model file; it is verified against those authoritative values. The
    other files carry no registry hash, so their computed SHA-256 is
    recorded (they are pinned by the registry `generated` revision).
    """
    output.mkdir(parents=True, exist_ok=True)
    recorded: dict[str, dict] = {}
    for key, info in resolved.items():
        gz_name = Path(info["path"]).name
        gz_path = output / gz_name
        print(f"download {info['url']}")
        download(info["url"], gz_path)
        if gz_name.endswith(".gz"):
            out_path = gz_path.with_suffix("")
            gunzip_to(gz_path, out_path)
            gz_path.unlink()
        else:
            out_path = gz_path

        actual = sha256_of(out_path)
        entry = {
            "name": out_path.name,
            "size_bytes": out_path.stat().st_size,
            "sha256": actual,
        }
        if "uncompressedHash" in info:
            expected_hash = info["uncompressedHash"]
            if actual != expected_hash:
                raise RuntimeError(
                    f"SHA-256 mismatch for {key}: expected {expected_hash}, "
                    f"got {actual}"
                )
            entry["registry_sha256"] = expected_hash
            print(f"  verified {key}: {actual} (registry)")
        else:
            print(f"  computed {key}: {actual}")
        if "uncompressedSize" in info:
            if out_path.stat().st_size != info["uncompressedSize"]:
                raise RuntimeError(
                    f"size mismatch for {key}: expected "
                    f"{info['uncompressedSize']} bytes, got "
                    f"{out_path.stat().st_size}"
                )
            entry["registry_size_bytes"] = info["uncompressedSize"]
        recorded[key] = entry
    return recorded


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Resolve and download the Mozilla Firefox en-zh "
        "Release (base-memory) Bergamot model.",
    )
    parser.add_argument(
        "--registry",
        default=REGISTRY_URL,
        help="Mozilla model registry URL (default: pinned production URL)",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("src-tauri/resources/models/translation/en-zh"),
        help="directory for the downloaded model files and per-pair "
        "download manifest",
    )
    parser.add_argument(
        "--download",
        action="store_true",
        help="download and verify the files (default: resolve + print plan)",
    )
    args = parser.parse_args()

    try:
        print(f"fetch registry: {args.registry}")
        registry = json.loads(fetch_bytes(args.registry))
        release = select_release(registry)
        plan, resolved = build_plan(registry, release)
        print(json.dumps(plan, ensure_ascii=False, indent=2))

        if args.download:
            recorded = verify_and_record(args.output, resolved)
            plan["files"] = recorded
            out_manifest = args.output / "manifest.json"
            out_manifest.write_text(
                json.dumps(plan, ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8",
            )
            total = sum(f["size_bytes"] for f in recorded.values())
            print(f"saved {len(recorded)} files to {args.output}")
            print(f"total size: {total:,} bytes ({total / 1_000_000:.2f} MB)")
            print(f"download manifest: {out_manifest}")
        return 0
    except (OSError, RuntimeError, ValueError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
