# Compact English/Japanese -> Chinese Translation Starter

Target:

- English -> Simplified Chinese: Mozilla Firefox Translations / Bergamot
- Japanese -> Simplified Chinese: shun89/opus-mt-ja-zh -> CTranslate2 INT8
- Translation model hard budget: 200 MB

This starter intentionally separates model preparation from the final application runtime.

## Files

- `config/translation.json`: recommended product defaults.
- `tools/fetch_firefox_enzh.py`: resolve current Mozilla en-zh Release model and download it.
- `tools/convert_ja_zh_ct2.sh`: convert MarianMT to CTranslate2 INT8.
- `tools/audit_model_sizes.py`: CI size gate.
- `python/reference_ja_zh.py`: baseline for Japanese -> Chinese.
- `bergamot/enzh.yml`: initial Bergamot configuration.
- `native/translation_bridge.h`: stable C ABI design.
- `rust/src/ffi.rs`: Rust declarations for the C ABI.
- `typescript/translation-client.ts`: frontend-facing translation API.

Important: the native bridge is an integration skeleton. You still need to link
Bergamot, CTranslate2, and SentencePiece for your target platform.
