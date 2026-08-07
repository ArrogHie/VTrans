"""Inspect PP-OCRv6 ONNX models and verify dict/class consistency.

Reads the real input/output names, dtypes, shapes and opset from the ONNX
files (never hard-codes node names, per the integration guide §5.3), then
checks that the recognition output class count C equals
dict_lines + blank + space (guide §9.4). Fails loudly on any mismatch and
writes a JSON report.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

import onnx
import onnxruntime as ort


def _dtype_name(elem_type: int) -> str:
    try:
        return onnx.TensorProto.DataType.Name(elem_type)
    except ValueError:
        return str(elem_type)


def _dims(value_info) -> list:
    dims = []
    for d in value_info.type.tensor_type.shape.dim:
        if d.HasField("dim_value"):
            dims.append(d.dim_value)
        elif d.HasField("dim_param"):
            dims.append(d.dim_param)
        else:
            dims.append("?")
    return dims


def inspect_model(path: Path) -> dict:
    model = onnx.load(str(path))
    opset = model.opset_import[0].version if model.opset_import else None
    inputs = [
        {
            "name": vi.name,
            "dtype": _dtype_name(vi.type.tensor_type.elem_type),
            "shape": _dims(vi),
        }
        for vi in model.graph.input
    ]
    outputs = [
        {
            "name": vi.name,
            "dtype": _dtype_name(vi.type.tensor_type.elem_type),
            "shape": _dims(vi),
        }
        for vi in model.graph.output
    ]

    # onnx.checker 与 ORT session 创建，确保模型可加载。
    onnx.checker.check_model(model)
    ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])

    return {
        "path": os.path.relpath(path).replace("\\", "/"),
        "size_bytes": path.stat().st_size,
        "opset": opset,
        "ir_version": model.ir_version,
        "inputs": inputs,
        "outputs": outputs,
        "ort_session_ok": True,
    }


def dict_lines(path: Path) -> int:
    return sum(1 for _ in path.open("r", encoding="utf-8"))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--det", required=True, type=Path)
    parser.add_argument("--rec", required=True, type=Path)
    parser.add_argument("--dict", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()

    det = inspect_model(args.det)
    rec = inspect_model(args.rec)

    lines = dict_lines(args.dict)
    # guide §9.2: characters = ["blank"] + dict_lines + [" "] when append_space=true
    # The blank index and space append flag come from the manifest defaults.
    expected_classes = lines + 2

    rec_output = rec["outputs"][0]
    actual_classes = rec_output["shape"][-1]
    if not isinstance(actual_classes, int):
        print(f"ERROR: rec output C is dynamic: {actual_classes}", file=sys.stderr)
        return 1

    report = {
        "det": det,
        "rec": rec,
        "dict": {
            "path": os.path.relpath(args.dict).replace("\\", "/"),
            "lines": lines,
            "expected_classes_with_blank_and_space": expected_classes,
        },
        "class_consistency": {
            "actual_classes": actual_classes,
            "expected_classes": expected_classes,
            "match": actual_classes == expected_classes,
        },
        "channel_order_note": (
            "BGR per PP-OCRv6 inference.yml DecodeImage.img_mode; "
            "the Python baseline is the authority."
        ),
    }

    print(json.dumps(report, indent=2, ensure_ascii=False))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")

    if not report["class_consistency"]["match"]:
        print(
            "ERROR: output C != dict lines + blank + space\n"
            f"  ONNX output shape: {rec_output['shape']}\n"
            f"  dict lines: {lines}\n"
            f"  expected classes: {expected_classes}\n"
            f"  actual classes: {actual_classes}\n"
            f"  dict path: {args.dict}",
            file=sys.stderr,
        )
        return 1

    print(
        f"OK: {args.det.name} / {args.rec.name} inspected; "
        f"class consistency ({actual_classes}) verified"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
