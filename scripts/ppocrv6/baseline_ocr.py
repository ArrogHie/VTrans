"""PP-OCRv6 Small ONNX Python baseline.

Reproduces the full det -> DB postprocess -> perspective crop -> rec -> CTC
pipeline against a fixed test image and saves intermediate artifacts
(det_input.npy, det_output.npy, det_boxes.json, crop_*.png,
rec_input_*.npy, rec_output_*.npy, result.json) so the Rust side can be
compared stage by stage (guide §14).

This is a v6-only baseline: it asserts correctness of the v6 models
themselves and never compares against PP-OCRv4.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

import cv2
import numpy as np
import onnxruntime as ort
import pyclipper


MEAN = np.array([0.485, 0.456, 0.406], dtype=np.float32)
STD = np.array([0.229, 0.224, 0.225], dtype=np.float32)
DET_THRESH = 0.2
BOX_THRESH = 0.45
MAX_CANDIDATES = 3000
UNCLIP_RATIO = 1.4
MIN_BOX_SIZE = 3.0
REC_HEIGHT = 48
REC_WIDTH = 320


def det_preprocess(image_bgr: np.ndarray) -> tuple[np.ndarray, np.ndarray, float, float]:
    """Resize to a 32-aligned limit (640 max side), normalize BGR, NCHW."""
    h, w = image_bgr.shape[:2]
    limit_side = 640
    ratio = min(limit_side / h, limit_side / w, 1.0)
    nh = max(32, int(round(h * ratio / 32)) * 32)
    nw = max(32, int(round(w * ratio / 32)) * 32)
    resized = cv2.resize(image_bgr, (nw, nh), interpolation=cv2.INTER_LINEAR)
    ratio_h = h / nh
    ratio_w = w / nw

    tensor = resized.astype(np.float32) / 255.0
    tensor = (tensor - MEAN) / STD
    tensor = tensor.transpose(2, 0, 1)[None].astype(np.float32)  # NCHW
    return tensor, resized, ratio_h, ratio_w


def unclip(poly: np.ndarray, distance: float) -> np.ndarray:
    pco = pyclipper.PyclipperOffset()
    pco.AddPath(poly.astype(np.int64).tolist(), pyclipper.JT_ROUND, pyclipper.ET_CLOSEDPOLYGON)
    offset = pco.Execute(distance)
    if not offset:
        return poly
    return np.array(offset[0], dtype=np.float32)


def box_score(prob: np.ndarray, poly: np.ndarray) -> float:
    h, w = prob.shape
    mask = np.zeros((h, w), dtype=np.uint8)
    cv2.fillPoly(mask, [poly.astype(np.int32)], 1)
    return float(prob[mask == 1].mean()) if mask.sum() else 0.0


def order_points(pts: np.ndarray) -> np.ndarray:
    """Return 4 points in clockwise order: top-left, top-right, bottom-right, bottom-left."""
    pts = pts.reshape(4, 2).astype(np.float32)
    s = pts.sum(axis=1)
    d = np.diff(pts, axis=1).ravel()
    tl = pts[np.argmin(s)]
    br = pts[np.argmax(s)]
    tr = pts[np.argmin(d)]
    bl = pts[np.argmax(d)]
    return np.array([tl, tr, br, bl], dtype=np.float32)


def db_postprocess(prob: np.ndarray, ratio_h: float, ratio_w: float) -> list[dict]:
    """DB postprocessing per guide §6.5, returning original-image boxes."""
    bitmap = (prob > DET_THRESH).astype(np.uint8)
    contours, _ = cv2.findContours(bitmap, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    results: list[dict] = []
    for contour in contours[:MAX_CANDIDATES]:
        rect = cv2.minAreaRect(contour)
        w, h = rect[1]
        if min(w, h) < MIN_BOX_SIZE:
            continue
        box = cv2.boxPoints(rect)
        score = box_score(prob, box)
        if score < BOX_THRESH:
            continue
        area = abs(cv2.contourArea(box))
        perimeter = cv2.arcLength(box, True)
        if perimeter <= 0:
            continue
        distance = area * UNCLIP_RATIO / perimeter
        expanded = unclip(box, distance)
        if len(expanded) < 4:
            continue
        final_rect = cv2.minAreaRect(expanded)
        final_box = cv2.boxPoints(final_rect)
        final_box = order_points(final_box)
        final_box[:, 0] *= ratio_w
        final_box[:, 1] *= ratio_h
        final_box = np.clip(final_box, 0, None)
        results.append({"points": final_box.tolist(), "score": score})
    return results


def crop_and_resize(image_bgr: np.ndarray, pts: np.ndarray) -> np.ndarray:
    """Perspective crop to a 48x<=320 line image with right zero padding."""
    tl, tr, br, bl = pts
    width = max(np.linalg.norm(tr - tl), np.linalg.norm(br - bl))
    height = max(np.linalg.norm(bl - tl), np.linalg.norm(br - tr))
    dst = np.array([[0, 0], [width - 1, 0], [width - 1, height - 1], [0, height - 1]], dtype=np.float32)
    matrix = cv2.getPerspectiveTransform(np.array([tl, tr, br, bl], dtype=np.float32), dst)
    warped = cv2.warpPerspective(image_bgr, matrix, (int(width), int(height)))
    # rotate tall crops to horizontal if needed
    if warped.shape[0] / max(warped.shape[1], 1) > 1.5:
        warped = cv2.rotate(warped, cv2.ROTATE_90_CLOCKWISE)

    h, w = warped.shape[:2]
    scale = REC_HEIGHT / h
    resized_w = min(int(round(w * scale)), REC_WIDTH)
    resized = cv2.resize(warped, (resized_w, REC_HEIGHT), interpolation=cv2.INTER_LINEAR)
    canvas = np.zeros((REC_HEIGHT, REC_WIDTH, 3), dtype=np.uint8)
    canvas[:, :resized_w] = resized
    return canvas


def rec_preprocess(crop_bgr: np.ndarray) -> np.ndarray:
    tensor = crop_bgr.astype(np.float32) / 255.0
    tensor = (tensor - 0.5) / 0.5
    tensor = tensor.transpose(2, 0, 1)[None].astype(np.float32)
    return tensor


def ctc_decode(logits: np.ndarray, characters: list[str], blank_index: int) -> tuple[str, float]:
    probs = np.exp(logits - logits.max(axis=-1, keepdims=True))
    probs /= probs.sum(axis=-1, keepdims=True)
    indices = probs.argmax(axis=-1)
    confs = probs.max(axis=-1)
    text: list[str] = []
    confs_kept: list[float] = []
    prev = blank_index
    for idx, conf in zip(indices, confs):
        if idx == blank_index or idx == prev:
            prev = idx
            continue
        text.append(characters[idx])
        confs_kept.append(float(conf))
        prev = idx
    score = float(np.mean(confs_kept)) if confs_kept else 0.0
    return "".join(text), score


def load_characters(dict_path: Path, append_space: bool = True) -> list[str]:
    chars = [line.rstrip("\n") for line in dict_path.open("r", encoding="utf-8")]
    return ["blank"] + chars + ([" "] if append_space else [])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--det", required=True, type=Path)
    parser.add_argument("--rec", required=True, type=Path)
    parser.add_argument("--dict", required=True, type=Path)
    parser.add_argument("--image", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()

    det_sess = ort.InferenceSession(str(args.det), providers=["CPUExecutionProvider"])
    rec_sess = ort.InferenceSession(str(args.rec), providers=["CPUExecutionProvider"])
    det_in = det_sess.get_inputs()[0].name
    det_out = det_sess.get_outputs()[0].name
    rec_in = rec_sess.get_inputs()[0].name
    rec_out = rec_sess.get_outputs()[0].name

    characters = load_characters(args.dict)
    image_bgr = cv2.imread(str(args.image), cv2.IMREAD_COLOR)
    if image_bgr is None:
        print(f"ERROR: cannot read image {args.image}", file=sys.stderr)
        return 1

    args.out.mkdir(parents=True, exist_ok=True)

    # det
    tensor, _, ratio_h, ratio_w = det_preprocess(image_bgr)
    np.save(args.out / "det_input.npy", tensor)
    prob_map = det_sess.run([det_out], {det_in: tensor})[0][0, 0]
    np.save(args.out / "det_output.npy", prob_map)
    boxes = db_postprocess(prob_map, ratio_h, ratio_w)
    (args.out / "det_boxes.json").write_text(
        json.dumps(boxes, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    # rec per box
    lines: list[dict] = []
    for i, box in enumerate(boxes):
        pts = np.array(box["points"], dtype=np.float32)
        crop = crop_and_resize(image_bgr, pts)
        cv2.imwrite(str(args.out / f"crop_{i:03d}.png"), crop)
        rec_tensor = rec_preprocess(crop)
        np.save(args.out / f"rec_input_{i:03d}.npy", rec_tensor)
        logits = rec_sess.run([rec_out], {rec_in: rec_tensor})[0][0]
        np.save(args.out / f"rec_output_{i:03d}.npy", logits)
        text, score = ctc_decode(logits, characters, blank_index=0)
        lines.append({"index": i, "text": text, "score": score, "points": box["points"]})

    result = {
        "model": "PP-OCRv6_small_det + PP-OCRv6_small_rec",
        "image": str(args.image),
        "det_input_shape": list(tensor.shape),
        "det_output_shape": list(prob_map.shape),
        "dict_lines": len(characters) - 2,
        "lines": lines,
    }
    (args.out / "result.json").write_text(
        json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
