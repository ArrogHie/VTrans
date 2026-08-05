# OCR test fixtures

This directory contains small test fixtures for the `vtrans-ocr` crate.

- `dict_ja.txt` and `dict_en.txt` are minimal character dictionaries used by
  unit and integration tests. The first line is the CTC blank.

Image fixtures are kept under 100 KB. Model files (`*.onnx`, `*.bin`) are
never committed to Git.

- `test1_lines.png` (96 KB) is a palette-compressed crop of the Wikipedia
  "Did you know" screenshot used to verify long-line recognition. It keeps the
  full horizontal resolution of the 1218 px source so characters stay large
  while the file stays under the 100 KB fixture limit. `test1_lines.txt` lists
  the expected logical lines (the bullet list wraps across several visual
  lines in the screenshot).
