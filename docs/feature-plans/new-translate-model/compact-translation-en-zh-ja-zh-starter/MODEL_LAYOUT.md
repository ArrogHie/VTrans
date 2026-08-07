# Recommended final layout

```text
models/
  ocr/
    det.onnx
    rec.onnx
  translation/
    en-zh/
      model.enzh.intgemm.alphas.bin
      srcvocab.enzh.spm
      trgvocab.enzh.spm
      lex.50.50.enzh.s2t.bin
      manifest.json
    ja-zh/
      model.bin
      config.json
      source_vocabulary.json
      target_vocabulary.json
      source.spm
      target.spm
      manifest.json
```

Run the size gate on `models/translation` in release CI.
