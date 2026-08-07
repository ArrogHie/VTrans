#!/usr/bin/env bash
set -euo pipefail

MODEL="${MODEL:-shun89/opus-mt-ja-zh}"
OUT="${OUT:-models/translation/ja-zh}"

rm -rf "$OUT"

ct2-transformers-converter \
  --model "$MODEL" \
  --output_dir "$OUT" \
  --quantization int8 \
  --copy_files source.spm target.spm tokenizer_config.json \
               special_tokens_map.json vocab.json

python "$(dirname "$0")/audit_model_sizes.py" "$OUT" --max-mb 110
echo "Japanese -> Chinese INT8 model prepared at: $OUT"
