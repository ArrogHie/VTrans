use std::ffi::{c_char, c_int, c_void, CStr, CString};

#[link(name = "translation_bridge")]
unsafe extern "C" {
    fn translation_create(
        enzh_model_dir: *const c_char,
        jazh_model_dir: *const c_char,
    ) -> *mut c_void;

    fn translation_translate(
        engine: *mut c_void,
        source_lang: *const c_char,
        input: *const c_char,
        output: *mut *mut c_char,
    ) -> c_int;

    fn translation_free_string(ptr: *mut c_char);
    fn translation_destroy(engine: *mut c_void);
}

pub struct NativeTranslator {
    ptr: *mut c_void,
}

unsafe impl Send for NativeTranslator {}

impl NativeTranslator {
    pub fn new(enzh: &str, jazh: &str) -> Result<Self, String> {
        let enzh = CString::new(enzh).map_err(|e| e.to_string())?;
        let jazh = CString::new(jazh).map_err(|e| e.to_string())?;
        let ptr = unsafe { translation_create(enzh.as_ptr(), jazh.as_ptr()) };
        if ptr.is_null() {
            return Err("translation_create failed".into());
        }
        Ok(Self { ptr })
    }

    pub fn translate(&self, lang: &str, text: &str) -> Result<String, String> {
        let lang = CString::new(lang).map_err(|e| e.to_string())?;
        let text = CString::new(text).map_err(|e| e.to_string())?;
        let mut output: *mut c_char = std::ptr::null_mut();

        let rc = unsafe {
            translation_translate(
                self.ptr,
                lang.as_ptr(),
                text.as_ptr(),
                &mut output,
            )
        };

        if rc != 0 || output.is_null() {
            return Err(format!("translation failed with code {rc}"));
        }

        let result = unsafe { CStr::from_ptr(output) }
            .to_string_lossy()
            .into_owned();

        unsafe { translation_free_string(output) };
        Ok(result)
    }
}

impl Drop for NativeTranslator {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { translation_destroy(self.ptr) };
            self.ptr = std::ptr::null_mut();
        }
    }
}
