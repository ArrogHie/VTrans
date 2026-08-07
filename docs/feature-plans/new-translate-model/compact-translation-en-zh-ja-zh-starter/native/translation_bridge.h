#pragma once
#include <stddef.h>

#ifdef _WIN32
#define TR_API __declspec(dllexport)
#else
#define TR_API __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef void* TranslationEngine;

/* Returns NULL on failure. Keep one engine alive for the application lifetime. */
TR_API TranslationEngine translation_create(
    const char* enzh_model_dir,
    const char* jazh_model_dir
);

/* source_lang must be "en" or "ja".
 * On success, *utf8_output is allocated by the bridge and must be released with
 * translation_free_string().
 * Returns 0 on success.
 */
TR_API int translation_translate(
    TranslationEngine engine,
    const char* source_lang,
    const char* utf8_input,
    char** utf8_output
);

TR_API void translation_free_string(char* ptr);
TR_API void translation_destroy(TranslationEngine engine);

#ifdef __cplusplus
}
#endif
