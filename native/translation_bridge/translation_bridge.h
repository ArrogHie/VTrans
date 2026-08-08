#pragma once
/*
 * translation_bridge.h - C ABI for the VTrans native translation engine.
 *
 * The bridge wraps Bergamot (en->zh) and CTranslate2 (ja->zh) behind a
 * tiny C interface so the Rust crate (`vtrans-translation`) never touches
 * C++ types. All text crosses the boundary as UTF-8; no system ACP/ANSI
 * conversion is performed anywhere in the bridge (see the integration
 * guide section 20).
 *
 * The four core functions (`translation_create`, `translation_translate`,
 * `translation_free_string`, `translation_destroy`) keep their documented
 * semantics. `translation_bridge_version` and `translation_set_quality`
 * are extensions used by the Rust side for ABI checking and quality
 * presets; removing them would still leave the core ABI intact.
 */

#include <stddef.h>

#ifdef _WIN32
#define TR_API __declspec(dllexport)
#else
#define TR_API __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque handle owned by the bridge; keep one engine for the application
 * lifetime (integration guide section 15). */
typedef void* TranslationEngine;

/* Error codes returned by bridge functions (integration guide section 21).
 * The Rust side maps these onto `vtrans_core::TranslationError` variants
 * without adding new variants. */
enum TranslationBridgeError {
    TR_OK = 0,
    TR_ERR_INVALID_ARGUMENT = 1,
    TR_ERR_UNSUPPORTED_LANGUAGE = 2,
    TR_ERR_MODEL_NOT_LOADED = 3,
    TR_ERR_TOKENIZER = 4,
    TR_ERR_INFERENCE = 5,
    TR_ERR_ENCODING = 6,
    TR_ERR_VERSION_MISMATCH = 7,
};

/* ABI version of this header. The Rust bindings refuse to load a DLL whose
 * version differs (mapped to TR_ERR_VERSION_MISMATCH / TranslationError::ModelLoad). */
#define TRANSLATION_BRIDGE_ABI_VERSION 1

/* Returns TRANSLATION_BRIDGE_ABI_VERSION. */
TR_API int translation_bridge_version(void);

/* Creates the engine from the two model directories:
 *   enzh_model_dir - Bergamot en->zh package (model + src/trg vocab + lexical shortlist)
 *   jazh_model_dir - CTranslate2 INT8 ja->zh package (model + config + vocab + spm)
 * Returns NULL on any failure (missing files, tokenizer/model load errors).
 * The engine is created with the "fast" quality preset; call
 * translation_set_quality() to switch presets before translating. */
TR_API TranslationEngine translation_create(
    const char* enzh_model_dir,
    const char* jazh_model_dir);

/* Sets the quality preset: "fast" or "balanced". Returns 0 on success or
 * a TR_ERR_* code (TR_ERR_INVALID_ARGUMENT for an unknown preset,
 * TR_ERR_MODEL_NOT_LOADED when the engine pointer is null). */
TR_API int translation_set_quality(TranslationEngine engine, const char* quality);

/* Translates UTF-8 `utf8_input` from `source_lang` ("en" or "ja").
 * On success returns 0 and stores a newly allocated UTF-8 string in
 * *utf8_output; the caller must release it with translation_free_string().
 * On failure returns a TR_ERR_* code and leaves *utf8_output untouched. */
TR_API int translation_translate(
    TranslationEngine engine,
    const char* source_lang,
    const char* utf8_input,
    char** utf8_output);

/* Releases a string previously returned by translation_translate. */
TR_API void translation_free_string(char* ptr);

/* Destroys the engine and releases all model resources. Safe to call with
 * a null pointer. */
TR_API void translation_destroy(TranslationEngine engine);

#ifdef __cplusplus
}
#endif
