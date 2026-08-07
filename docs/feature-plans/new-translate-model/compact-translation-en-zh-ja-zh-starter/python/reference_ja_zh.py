#!/usr/bin/env python3
from __future__ import annotations
import argparse
import ctranslate2
import sentencepiece as spm

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--source-spm", required=True)
    ap.add_argument("--target-spm", required=True)
    ap.add_argument("--text", required=True)
    ap.add_argument("--beam-size", type=int, default=4)
    ap.add_argument("--threads", type=int, default=2)
    args = ap.parse_args()

    src = spm.SentencePieceProcessor(model_file=args.source_spm)
    tgt = spm.SentencePieceProcessor(model_file=args.target_spm)

    translator = ctranslate2.Translator(
        args.model,
        device="cpu",
        compute_type="int8",
        inter_threads=1,
        intra_threads=args.threads,
    )

    tokens = src.encode(args.text, out_type=str)
    if len(tokens) > 256:
        tokens = tokens[:256]

    result = translator.translate_batch(
        [tokens],
        beam_size=args.beam_size,
        max_decoding_length=320,
    )[0]

    text = tgt.decode(result.hypotheses[0])
    print(text)

if __name__ == "__main__":
    main()
