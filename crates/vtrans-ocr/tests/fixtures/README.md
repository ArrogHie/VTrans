# OCR test fixtures

This directory contains small test fixtures for the `vtrans-ocr` crate.

- `dict_ja.txt` and `dict_en.txt` are minimal character dictionaries used by
  unit and integration tests. The first line is the CTC blank.

Image fixtures are kept under 100 KB. Model files (`*.onnx`, `*.bin`) are
never committed to Git.
