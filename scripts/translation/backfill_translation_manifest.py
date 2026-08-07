#!/usr/bin/env python3
"""Backfill the translation section of the v2 model manifest.

Measures SHA-256 / size_bytes of the ten dual-engine translation files
(4 Bergamot en-zh + 6 CTranslate2 ja-zh), verifies every file exists and
every hash is real (never writes placeholder values), and updates the
`translation` section of `src-tauri/resources/models/manifest.json` (and
the crate template `crates/vtrans-models/resources/manifest.json` when
`--update-template` is given). The OCR section is left untouched.

Provenance metadata is carried from the download/conversion artifacts:
`--en-zh-download` (per-pair manifest written by fetch_firefox_enzh.py)
and `--ja-zh-meta` (JSON written by convert_ja_zh_ct2.ps1). Extra
`--metadata key=value` pairs can be added and override the defaults.

Exit codes:
  0  success
  1  missing file / placeholder hash / invalid manifest
  2  usage error
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

EN_ZH_FILES = {
    "model": "model.enzh.intgemm.alphas.bin",
    "src_vocab": "srcvocab.enzh.spm",
    "trg_vocab": "trgvocab.enzh.spm",
    "lexical_shortlist": "lex.50.50.enzh.s2t.bin",
}
JA_ZH_FILES = {
    "model": "model.bin",
    "config": "config.json",
    "source_vocabulary": "source_vocabulary.json",
    "target_vocabulary": "target_vocabulary.json",
    "source_spm": "source.spm",
    "target_spm": "target.spm",
}

SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def entry_from(slot: str, rel_path: str, full: Path, entry_id: str) -> dict:
    if not full.is_file():
        raise RuntimeError(f"required file missing: {full}")
    digest = sha256_of(full)
    if not SHA256_RE.match(digest):
        raise RuntimeError(f"invalid sha256 for {full}: {digest!r}")
    size = full.stat().st_size
    if size <= 0:
        raise RuntimeError(f"empty file for {full}; refusing to backfill")
    return {
        "id": entry_id,
        "path": rel_path,
        "sha256": digest,
        "size_bytes": size,
    }


def build_translation_section(
    en_zh_dir: Path,
    ja_zh_dir: Path,
    en_zh_download: Path | None,
    ja_zh_meta: Path | None,
    extra_metadata: dict[str, str],
) -> dict:
    en_zh_engines = {}
    for slot, name in EN_ZH_FILES.items():
        rel = f"translation/en-zh/{name}"
        en_zh_engines[slot] = entry_from(
            slot, rel, en_zh_dir / name, f"enzh-{slot.replace('_', '-')}"
        )
    ja_zh_engines = {}
    for slot, name in JA_ZH_FILES.items():
        rel = f"translation/ja-zh/{name}"
        ja_zh_engines[slot] = entry_from(
            slot, rel, ja_zh_dir / name, f"jazh-{slot.replace('_', '-')}"
        )

    metadata: dict[str, str] = {
        "model_source": "shun89/opus-mt-ja-zh",
        "quantization": "int8",
    }
    if en_zh_download is not None and en_zh_download.is_file():
        plan = json.loads(en_zh_download.read_text(encoding="utf-8"))
        for key in (
            "registry",
            "registryGenerated",
            "architecture",
            "releaseStatus",
            "revision",
        ):
            if plan.get(key):
                metadata[key] = str(plan[key])
        # 语义化 key：en-zh 的 revision 即 Bergamot 模型版本。
        if plan.get("revision"):
            metadata["model_revision"] = str(plan["revision"])
    if ja_zh_meta is not None and ja_zh_meta.is_file():
        meta = json.loads(ja_zh_meta.read_text(encoding="utf-8"))
        if meta.get("model_revision"):
            metadata["ct2_model_revision"] = str(meta["model_revision"])
        if meta.get("converted_with"):
            metadata["converted_with"] = str(meta["converted_with"])
        if meta.get("quantization"):
            metadata["quantization"] = str(meta["quantization"])

    metadata.update(extra_metadata)

    return {
        "target": "zh-Hans",
        "engines": {
            "en_zh": {
                "engine": "bergamot",
                **en_zh_engines,
                "beam_size": 1,
                "gemm_precision": "int8shiftAlphaAll",
            },
            "ja_zh": {
                "engine": "ctranslate2",
                **ja_zh_engines,
                "beam_size_fast": 1,
                "beam_size_balanced": 4,
                "max_input_tokens": 256,
            },
        },
        "budget_mb": {
            "hard_mb": 200,
            "target_mb": 175,
            "en_zh_mb": 65,
            "ja_zh_mb": 110,
        },
        "metadata": dict(sorted(metadata.items())),
    }


def compact(obj) -> str:
    return json.dumps(obj, ensure_ascii=False, separators=(",", ": "))


def render(node, indent: int) -> str:
    """Render JSON in the compact style used by the repo manifests."""
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


def backfill(manifest_path: Path, section: dict) -> None:
    if not manifest_path.is_file():
        raise RuntimeError(f"manifest not found: {manifest_path}")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("version") != 2:
        raise RuntimeError(
            f"manifest {manifest_path} is not v2 (version="
            f"{manifest.get('version')!r}); refusing to backfill"
        )
    if "ocr" not in manifest:
        raise RuntimeError(f"manifest {manifest_path} has no ocr section")
    manifest["translation"] = section
    manifest_path.write_text(
        render(manifest, 0) + "\n",
        encoding="utf-8",
    )


def parse_metadata(pairs: list[str]) -> dict[str, str]:
    result = {}
    for pair in pairs:
        if "=" not in pair:
            raise ValueError(f"metadata must be key=value, got {pair!r}")
        key, value = pair.split("=", 1)
        result[key] = value
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--manifest",
        type=Path,
        default=Path("src-tauri/resources/models/manifest.json"),
        help="v2 model manifest to update",
    )
    parser.add_argument(
        "--en-zh-dir",
        type=Path,
        default=Path("src-tauri/resources/models/translation/en-zh"),
        help="Bergamot en-zh directory",
    )
    parser.add_argument(
        "--ja-zh-dir",
        type=Path,
        default=Path("src-tauri/resources/models/translation/ja-zh"),
        help="CTranslate2 ja-zh directory",
    )
    parser.add_argument(
        "--en-zh-download",
        type=Path,
        help="per-pair download manifest from fetch_firefox_enzh.py "
        "(default: <en-zh-dir>/manifest.json)",
    )
    parser.add_argument(
        "--ja-zh-meta",
        type=Path,
        help="conversion metadata JSON from convert_ja_zh_ct2.ps1 "
        "(default: scripts/translation/work/ja-zh-meta.json)",
    )
    parser.add_argument(
        "--update-template",
        action="store_true",
        help="also update crates/vtrans-models/resources/manifest.json",
    )
    parser.add_argument(
        "--metadata",
        action="append",
        default=[],
        metavar="KEY=VALUE",
        help="extra manifest metadata (repeatable; overrides defaults)",
    )
    args = parser.parse_args()

    en_zh_download = args.en_zh_download or (args.en_zh_dir / "manifest.json")
    ja_zh_meta = args.ja_zh_meta or Path(
        "scripts/translation/work/ja-zh-meta.json"
    )
    try:
        extra = parse_metadata(args.metadata)
        section = build_translation_section(
            args.en_zh_dir,
            args.ja_zh_dir,
            en_zh_download,
            ja_zh_meta,
            extra,
        )
        backfill(args.manifest, section)
        print(f"backfilled {args.manifest}")
        print(f"  translation.target = {section['target']}")
        en_zh_model = section["engines"]["en_zh"]["model"]["sha256"]
        ja_zh_model = section["engines"]["ja_zh"]["model"]["sha256"]
        print(f"  en-zh model sha256 = {en_zh_model}")
        print(f"  ja-zh model sha256 = {ja_zh_model}")
        print(f"  metadata = {json.dumps(section['metadata'], ensure_ascii=False)}")
        if args.update_template:
            template = Path(
                "crates/vtrans-models/resources/manifest.json"
            )
            backfill(template, section)
            print(f"backfilled {template}")
        return 0
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
