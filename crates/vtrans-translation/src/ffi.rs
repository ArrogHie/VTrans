//! FFI bindings for the native translation bridge (`translation_bridge.dll`).
//!
//! The bridge is loaded dynamically with `libloading`; there is no
//! link-time dependency on the DLL, so the crate still compiles when the
//! native artifact is absent (the error is reported at runtime as
//! [`TranslationError::ModelLoad`]).
//!
//! All strings cross the FFI boundary as UTF-8. The bridge owns the two
//! models for the process lifetime; this wrapper owns the opaque engine
//! handle and releases it in [`Drop`].

#![allow(unsafe_code)] // FFI interop requires unsafe; every block below has a `// SAFETY:` comment.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libloading::{Library, Symbol};
use tracing::debug;

use vtrans_core::types::Language;
use vtrans_core::TranslationError;

/// ABI version expected by these bindings. Must match
/// `TRANSLATION_BRIDGE_ABI_VERSION` in `native/translation_bridge/translation_bridge.h`.
pub const TRANSLATION_BRIDGE_ABI_VERSION: i32 = 1;

/// File name of the bridge library on Windows.
#[cfg(windows)]
pub const BRIDGE_LIBRARY_NAME: &str = "translation_bridge.dll";
/// File name of the bridge library on Unix-like platforms.
#[cfg(not(windows))]
pub const BRIDGE_LIBRARY_NAME: &str = "libtranslation_bridge.so";

type CreateFn = unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_void;
type SetQualityFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int;
type TranslateFn =
    unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *mut *mut c_char) -> c_int;
type FreeStringFn = unsafe extern "C" fn(*mut c_char);
type DestroyFn = unsafe extern "C" fn(*mut c_void);
type VersionFn = unsafe extern "C" fn() -> c_int;

/// Wrapper around a loaded native translation engine.
///
/// The wrapper is `Send + Sync`: the bridge serializes all calls behind an
/// internal mutex and keeps the thread budget fixed (`Bergamot` 2 threads,
/// `CTranslate2` intra 2 / inter 1), so sharing one engine across the
/// application is safe. The underlying engine is released on drop.
pub struct NativeTranslator {
    engine: *mut c_void,
    set_quality: SetQualityFn,
    translate: TranslateFn,
    free_string: FreeStringFn,
    destroy: DestroyFn,
    // The library must outlive every function pointer and the engine;
    // field order guarantees it is dropped last.
    _library: Arc<Library>,
}

// SAFETY justification: the engine handle is opaque to Rust and all access
// goes through the C functions, which the bridge serializes with an
// internal mutex. The library reference keeps the code alive for the whole
// wrapper lifetime. Dropping the wrapper is the only owner, so no other
// thread can race a destroy.
unsafe impl Send for NativeTranslator {}
// SAFETY justification: see `Send`; all C functions are safe to call from
// any thread because the bridge takes the engine mutex internally.
unsafe impl Sync for NativeTranslator {}

impl NativeTranslator {
    /// Load the bridge library, verify its ABI version, and create the
    /// engine from the two model directories.
    ///
    /// # Arguments
    ///
    /// * `library_path` - Path of `translation_bridge.dll`.
    /// * `enzh_dir` - `Bergamot` en→zh model directory.
    /// * `jazh_dir` - `CTranslate2` INT8 ja→zh model directory.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::ModelLoad`] when the library cannot be
    /// loaded, a symbol is missing, the ABI version mismatches, or the
    /// bridge fails to create the engine.
    #[tracing::instrument(skip_all, fields(library = %library_path.display()))]
    pub fn load(
        library_path: &Path,
        enzh_dir: &Path,
        jazh_dir: &Path,
    ) -> Result<Self, TranslationError> {
        // SAFETY: libloading keeps the library mapped for as long as the
        // `Library` value is alive; we keep it in `_library` until the
        // wrapper is dropped, and only then release the engine.
        let library = unsafe { Library::new(library_path) }.map_err(|error| {
            TranslationError::ModelLoad(format!(
                "failed to load {}: {error}",
                library_path.display()
            ))
        })?;
        let library = Arc::new(library);

        // SAFETY: `get` returns a symbol pointer valid while `library` is
        // alive; the call itself is safe. Symbol names are fixed by the C
        // header, not user-controlled.
        let version: Symbol<VersionFn> = unsafe { library.get(b"translation_bridge_version") }
            .map_err(|error| {
                TranslationError::ModelLoad(format!(
                    "translation_bridge missing symbol translation_bridge_version: {error}"
                ))
            })?;
        let create: Symbol<CreateFn> =
            unsafe { library.get(b"translation_create") }.map_err(|error| {
                TranslationError::ModelLoad(format!(
                    "translation_bridge missing symbol translation_create: {error}"
                ))
            })?;
        let set_quality: Symbol<SetQualityFn> = unsafe { library.get(b"translation_set_quality") }
            .map_err(|error| {
                TranslationError::ModelLoad(format!(
                    "translation_bridge missing symbol translation_set_quality: {error}"
                ))
            })?;
        let translate: Symbol<TranslateFn> = unsafe { library.get(b"translation_translate") }
            .map_err(|error| {
                TranslationError::ModelLoad(format!(
                    "translation_bridge missing symbol translation_translate: {error}"
                ))
            })?;
        let free_string: Symbol<FreeStringFn> = unsafe { library.get(b"translation_free_string") }
            .map_err(|error| {
                TranslationError::ModelLoad(format!(
                    "translation_bridge missing symbol translation_free_string: {error}"
                ))
            })?;
        let destroy: Symbol<DestroyFn> =
            unsafe { library.get(b"translation_destroy") }.map_err(|error| {
                TranslationError::ModelLoad(format!(
                    "translation_bridge missing symbol translation_destroy: {error}"
                ))
            })?;

        // SAFETY: calling a version function with no arguments is safe; it
        // only returns an integer constant.
        let reported_version = unsafe { version() };
        if reported_version != TRANSLATION_BRIDGE_ABI_VERSION {
            return Err(TranslationError::ModelLoad(format!(
                "translation_bridge ABI version mismatch: expected {TRANSLATION_BRIDGE_ABI_VERSION}, \
                 got {reported_version}"
            )));
        }

        let enzh_c = path_to_cstring(enzh_dir)?;
        let jazh_c = path_to_cstring(jazh_dir)?;
        // SAFETY: the engine is created with valid NUL-terminated paths and
        // the returned handle is owned by this wrapper. A null handle means
        // the bridge failed to load the models.
        let engine = unsafe { create(enzh_c.as_ptr(), jazh_c.as_ptr()) };
        if engine.is_null() {
            return Err(TranslationError::ModelLoad(format!(
                "translation_create failed for enzh={} jazh={}",
                enzh_dir.display(),
                jazh_dir.display()
            )));
        }

        debug!(
            enzh = %enzh_dir.display(),
            jazh = %jazh_dir.display(),
            "native translation engine created"
        );
        Ok(Self {
            engine,
            set_quality: *set_quality,
            translate: *translate,
            free_string: *free_string,
            destroy: *destroy,
            _library: library,
        })
    }

    /// Switch the quality preset of the engine.
    ///
    /// # Arguments
    ///
    /// * `quality` - `"fast"` or `"balanced"` (see [`crate::native::TranslationQuality`]).
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::ModelLoad`] when the bridge rejects the
    /// preset or cannot rebuild the Bergamot model.
    pub fn set_quality(&self, quality: &str) -> Result<(), TranslationError> {
        let quality_c = CString::new(quality).map_err(|_| {
            TranslationError::Inference("quality string contains a NUL byte".to_string())
        })?;
        // SAFETY: `self.engine` is a valid handle owned by this wrapper and
        // the quality string is NUL-terminated; the bridge serializes the
        // call internally.
        let rc = unsafe { (self.set_quality)(self.engine, quality_c.as_ptr()) };
        if rc == 0 {
            Ok(())
        } else {
            Err(TranslationError::ModelLoad(format!(
                "translation_set_quality failed with bridge code {rc}"
            )))
        }
    }

    /// Translate `text` from `source` (a blocking native call).
    ///
    /// The call is not interruptible once it enters the bridge (decision
    /// B3); callers must run it in `spawn_blocking` and check the
    /// cancellation token before and after the call.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::UnsupportedPair`] for any source other
    /// than `en`/`ja`, [`TranslationError::Inference`] for invalid input
    /// and bridge failures, and [`TranslationError::ModelLoad`] when the
    /// engine is not usable.
    pub fn translate_blocking(
        &self,
        source: Language,
        text: &str,
    ) -> Result<String, TranslationError> {
        let lang = validate_translate_call(source, text)?;
        let lang_c = CString::new(lang)
            .map_err(|_| TranslationError::Inference("invalid language".to_string()))?;
        let text_c = CString::new(text).map_err(|_| {
            TranslationError::Inference("source text contains a NUL byte".to_string())
        })?;

        let mut output: *mut c_char = std::ptr::null_mut();
        // SAFETY: the engine handle is valid and owned by this wrapper; all
        // input pointers are valid NUL-terminated UTF-8 buffers. The bridge
        // only writes to `output` on success, and `output` is released
        // below before it can escape this scope.
        let rc =
            unsafe { (self.translate)(self.engine, lang_c.as_ptr(), text_c.as_ptr(), &mut output) };
        if rc != 0 {
            return Err(map_bridge_error(rc, source, Language::ChineseSimplified));
        }
        let translated = decode_output(output)?;

        // SAFETY: `output` was allocated by the bridge and must be released
        // exactly once with its free function.
        unsafe { (self.free_string)(output) };
        Ok(translated)
    }
}

impl Drop for NativeTranslator {
    fn drop(&mut self) {
        if !self.engine.is_null() {
            // SAFETY: the handle was created by `translation_create` and is
            // destroyed exactly once here; the library is still mapped
            // because `_library` is dropped after this method returns.
            unsafe { (self.destroy)(self.engine) };
            self.engine = std::ptr::null_mut();
        }
    }
}

/// Locate the bridge library next to the models directory or in the
/// packaged resources.
///
/// Candidate order:
/// 1. `<models_dir>/../native/translation_bridge.dll` (repo/package layout)
/// 2. `<models_dir>/native/translation_bridge.dll`
/// 3. `<current_exe>/native/translation_bridge.dll`
/// 4. `<current_exe>/resources/native/translation_bridge.dll` (Tauri bundle)
///
/// # Errors
///
/// Returns [`TranslationError::ModelLoad`] when no candidate exists.
pub(crate) fn locate_library(models_dir: &Path) -> Result<PathBuf, TranslationError> {
    let mut candidates = Vec::new();
    if let Some(parent) = models_dir.parent() {
        candidates.push(parent.join("native").join(BRIDGE_LIBRARY_NAME));
    }
    candidates.push(models_dir.join("native").join(BRIDGE_LIBRARY_NAME));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("native").join(BRIDGE_LIBRARY_NAME));
            candidates.push(
                exe_dir
                    .join("resources")
                    .join("native")
                    .join(BRIDGE_LIBRARY_NAME),
            );
        }
    }

    for candidate in &candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }
    Err(TranslationError::ModelLoad(format!(
        "{} not found; looked at: {}",
        BRIDGE_LIBRARY_NAME,
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

/// Convert a filesystem path to a NUL-terminated UTF-8 string for the C
/// boundary. Non-UTF-8 paths are replaced lossily; the bridge is only
/// passed paths from the manifest, which is JSON (UTF-8) by construction.
fn path_to_cstring(path: &Path) -> Result<CString, TranslationError> {
    CString::new(path.to_string_lossy().as_bytes()).map_err(|_| {
        TranslationError::ModelLoad(format!(
            "model path contains a NUL byte: {}",
            path.display()
        ))
    })
}

/// Map a bridge error code onto [`TranslationError`] (integration guide
/// section 21). The mapping reuses existing core variants and does not add
/// new ones.
///
/// # Mapping
///
/// | bridge code | meaning | mapped error |
/// |------------:|---------|--------------|
/// | 1 | invalid argument | `Inference` |
/// | 2 | unsupported language | `UnsupportedPair { src, target }` |
/// | 3 | model not loaded | `ModelLoad` |
/// | 4 | tokenizer failure | `ModelLoad` |
/// | 5 | inference failure | `Inference` |
/// | 6 | output encoding failure | `Inference` |
/// | 7 | version mismatch | `ModelLoad` |
/// | other | unknown | `Inference` |
pub(crate) fn map_bridge_error(code: i32, src: Language, target: Language) -> TranslationError {
    match code {
        1 => TranslationError::Inference("native bridge: invalid argument".to_string()),
        2 => TranslationError::UnsupportedPair { src, target },
        3 => TranslationError::ModelLoad("native bridge: model not loaded".to_string()),
        4 => TranslationError::ModelLoad("native bridge: tokenizer failure".to_string()),
        5 => TranslationError::Inference("native bridge: inference failure".to_string()),
        6 => TranslationError::Inference("native bridge: output encoding failure".to_string()),
        7 => TranslationError::ModelLoad("native bridge: ABI version mismatch".to_string()),
        other => TranslationError::Inference(format!("native bridge error code {other}")),
    }
}

/// Validate a translate call before it reaches the FFI boundary.
///
/// Rejects empty text and any source other than `en`/`ja` (the bridge has
/// exactly these two engines; `Auto` must be resolved by the caller).
///
/// # Errors
///
/// Returns [`TranslationError::Inference`] for empty text and
/// [`TranslationError::UnsupportedPair`] for unsupported sources.
fn validate_translate_call(source: Language, text: &str) -> Result<&'static str, TranslationError> {
    if text.is_empty() {
        return Err(TranslationError::Inference("empty source text".to_string()));
    }
    match source {
        Language::English => Ok("en"),
        Language::Japanese => Ok("ja"),
        _ => Err(TranslationError::UnsupportedPair {
            src: source,
            target: Language::ChineseSimplified,
        }),
    }
}

/// Decode a bridge output pointer into a Rust string.
///
/// Returns [`TranslationError::Inference`] when the pointer is null or the
/// bytes are not valid UTF-8. The caller is responsible for releasing the
/// pointer with the bridge's free function after a successful decode.
fn decode_output(output: *mut c_char) -> Result<String, TranslationError> {
    if output.is_null() {
        return Err(TranslationError::Inference(
            "native bridge returned null output".to_string(),
        ));
    }
    // SAFETY: the bridge guarantees `output` is a valid NUL-terminated
    // UTF-8 string on success; the caller keeps the allocation alive until
    // this function returns.
    let translated = unsafe { CStr::from_ptr(output) }
        .to_str()
        .map_err(|_| {
            TranslationError::Inference("native bridge output is not valid UTF-8".to_string())
        })?
        .to_string();
    Ok(translated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtrans_core::TranslationError;

    #[test]
    fn bridge_error_code_mapping_covers_0_to_7() {
        let src = Language::English;
        let target = Language::ChineseSimplified;
        // 0 (OK) is not expected here, but the mapper must not panic on it.
        assert!(matches!(
            map_bridge_error(0, src, target),
            TranslationError::Inference(_)
        ));
        assert!(matches!(
            map_bridge_error(1, src, target),
            TranslationError::Inference(_)
        ));
        assert!(matches!(
            map_bridge_error(2, src, target),
            TranslationError::UnsupportedPair { .. }
        ));
        assert!(matches!(
            map_bridge_error(3, src, target),
            TranslationError::ModelLoad(_)
        ));
        assert!(matches!(
            map_bridge_error(4, src, target),
            TranslationError::ModelLoad(_)
        ));
        assert!(matches!(
            map_bridge_error(5, src, target),
            TranslationError::Inference(_)
        ));
        assert!(matches!(
            map_bridge_error(6, src, target),
            TranslationError::Inference(_)
        ));
        assert!(matches!(
            map_bridge_error(7, src, target),
            TranslationError::ModelLoad(_)
        ));
        assert!(matches!(
            map_bridge_error(99, src, target),
            TranslationError::Inference(_)
        ));
    }

    #[test]
    fn unsupported_language_code_carries_the_requested_pair() {
        let error = map_bridge_error(2, Language::English, Language::ChineseSimplified);
        assert!(matches!(
            error,
            TranslationError::UnsupportedPair {
                src: Language::English,
                target: Language::ChineseSimplified,
            }
        ));
    }

    #[test]
    fn error_messages_are_stable_and_descriptive() {
        assert!(
            map_bridge_error(3, Language::English, Language::ChineseSimplified)
                .to_string()
                .contains("model not loaded")
        );
        assert!(
            map_bridge_error(5, Language::Japanese, Language::ChineseSimplified)
                .to_string()
                .contains("inference failure")
        );
        assert!(
            map_bridge_error(7, Language::Japanese, Language::ChineseSimplified)
                .to_string()
                .contains("version mismatch")
        );
    }

    #[test]
    fn validate_translate_call_rejects_auto_and_unsupported_sources() {
        let err = validate_translate_call(Language::Auto, "hello").unwrap_err();
        assert!(matches!(
            err,
            TranslationError::UnsupportedPair {
                src: Language::Auto,
                ..
            }
        ));
        let err = validate_translate_call(Language::ChineseSimplified, "hello").unwrap_err();
        assert!(matches!(
            err,
            TranslationError::UnsupportedPair {
                src: Language::ChineseSimplified,
                ..
            }
        ));
    }

    #[test]
    fn validate_translate_call_accepts_supported_sources() {
        assert_eq!(
            validate_translate_call(Language::English, "hello").unwrap(),
            "en"
        );
        assert_eq!(
            validate_translate_call(Language::Japanese, "こんにちは").unwrap(),
            "ja"
        );
    }

    #[test]
    fn validate_translate_call_rejects_empty_text() {
        assert!(matches!(
            validate_translate_call(Language::English, "").unwrap_err(),
            TranslationError::Inference(_)
        ));
    }

    #[test]
    fn decode_output_rejects_null_pointer() {
        assert!(matches!(
            decode_output(std::ptr::null_mut()).unwrap_err(),
            TranslationError::Inference(_)
        ));
    }

    #[test]
    fn decode_output_rejects_invalid_utf8() {
        // A NUL-terminated byte sequence that is not valid UTF-8.
        let bytes = CString::new(vec![0xFF_u8, 0xFE_u8]).unwrap();
        let ptr = bytes.into_raw();
        let error = decode_output(ptr).unwrap_err();
        assert!(matches!(error, TranslationError::Inference(_)));
        // SAFETY: `ptr` came from `CString::into_raw` and is returned to it
        // exactly once here.
        unsafe { drop(CString::from_raw(ptr)) };
    }

    #[test]
    fn decode_output_reads_utf8_string() {
        let bytes = CString::new("你好").unwrap();
        let ptr = bytes.into_raw();
        let decoded = decode_output(ptr).unwrap();
        assert_eq!(decoded, "你好");
        // SAFETY: see `decode_output_rejects_invalid_utf8`.
        unsafe { drop(CString::from_raw(ptr)) };
    }

    #[test]
    fn locate_library_prefers_models_sibling_native_dir() {
        let temp = std::env::temp_dir().join("vtrans-locate-test");
        let models = temp.join("models");
        let native = temp.join("native");
        std::fs::create_dir_all(&native).unwrap();
        let dll = native.join(BRIDGE_LIBRARY_NAME);
        std::fs::write(&dll, b"stub").unwrap();

        let found = locate_library(&models).unwrap();
        assert_eq!(found, dll);
        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn locate_library_reports_missing_with_candidates() {
        let temp = std::env::temp_dir().join("vtrans-locate-missing-test");
        let models = temp.join("models");
        let error = locate_library(&models).unwrap_err();
        assert!(matches!(error, TranslationError::ModelLoad(_)));
        assert!(error.to_string().contains(BRIDGE_LIBRARY_NAME));
    }
}
