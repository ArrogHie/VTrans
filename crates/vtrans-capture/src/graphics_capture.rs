#![allow(unsafe_code)] // Windows interop requires unsafe; each block below has a SAFETY comment.

//! Low-level Windows Graphics Capture API wrapper.
//!
//! This module encapsulates the Direct3D 11 device creation, `WinRT`
//! `GraphicsCaptureItem` creation from an `HMONITOR`, frame pool
//! management, and pixel-data extraction from captured frames. All
//! `unsafe` code is confined to this module and the `source`/`session`
//! modules work through the safe abstractions defined here.

use std::cell::Cell;
use std::sync::{Arc, Mutex};

use vtrans_core::types::{CapturedImage, PixelFormat};
use vtrans_core::CaptureError;
use windows::core::Interface;
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::{IDirect3DDevice, IDirect3DSurface};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{D3D11CreateDevice, D3D11_SDK_VERSION};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

thread_local! {
    static WINRT_INITIALIZED: Cell<bool> = const { Cell::new(false) };
}

/// Builds an `InitFailed` error after logging the underlying cause.
fn init_failed(context: &'static str, error: impl std::fmt::Display) -> CaptureError {
    tracing::warn!(error = %error, context, "capture initialization failed");
    CaptureError::InitFailed(format!("{context}: {error}"))
}

/// Builds a `FrameGrabFailed` error after logging the underlying cause.
fn frame_failed(context: &'static str, error: impl std::fmt::Display) -> CaptureError {
    tracing::warn!(error = %error, context, "frame grab failed");
    CaptureError::FrameGrabFailed(format!("{context}: {error}"))
}

/// Initializes the `WinRT` apartment once per thread.
///
/// Windows Graphics Capture activation requires `RoInitialize` to be called
/// on the current thread. The initialization is intentionally never undone:
/// Tokio worker threads are long-lived, and uninitializing could disturb a
/// COM apartment that was already set up by the host application.
fn ensure_winrt_initialized() -> Result<(), CaptureError> {
    WINRT_INITIALIZED.with(|initialized| {
        if initialized.get() {
            return Ok(());
        }

        // SAFETY: `RO_INIT_MULTITHREADED` is a valid apartment mode and the
        // call has no pointer arguments or external lifetime requirements.
        let result = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        match result {
            Ok(()) => {
                initialized.set(true);
                Ok(())
            }
            Err(e) => Err(init_failed("RoInitialize", e)),
        }
    })
}

/// D3D 11 device and immediate context, plus the `WinRT` Direct3D device
/// needed by the Graphics Capture API.
///
/// Created once and shared (via COM reference-counting) across all
/// capture operations.
pub(crate) struct D3D11Context {
    device: ID3D11Device,
    context: Arc<Mutex<ID3D11DeviceContext>>,
    winrt_device: IDirect3DDevice,
}

// SAFETY: The D3D11 immediate context is guarded by a `Mutex`; the device
// and `WinRT` device are reference-counted and safe to share across threads
// in the Multi-Threaded Apartment (MTA).
unsafe impl Send for D3D11Context {}
unsafe impl Sync for D3D11Context {}

impl D3D11Context {
    /// Creates a new D3D 11 device with BGRA support.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::InitFailed`] if the D3D 11 device cannot
    /// be created or the `WinRT` interop fails.
    #[tracing::instrument]
    pub(crate) fn new() -> Result<Self, CaptureError> {
        let (device, context) = create_d3d11_device()?;
        let winrt_device = create_winrt_device(&device)?;
        tracing::debug!("D3D11 context initialized for graphics capture");
        Ok(Self {
            device,
            context: Arc::new(Mutex::new(context)),
            winrt_device,
        })
    }

    pub(crate) fn device(&self) -> ID3D11Device {
        self.device.clone()
    }
    pub(crate) fn context(&self) -> Arc<Mutex<ID3D11DeviceContext>> {
        self.context.clone()
    }
    pub(crate) fn winrt_device(&self) -> IDirect3DDevice {
        self.winrt_device.clone()
    }
}

fn create_d3d11_device() -> Result<(ID3D11Device, ID3D11DeviceContext), CaptureError> {
    let mut last_error: Option<CaptureError> = None;
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        match create_d3d11_device_with_driver(driver) {
            Ok(ok) => return Ok(ok),
            Err(e) => last_error = Some(e),
        }
    }

    Err(last_error.unwrap_or_else(|| init_failed("D3D11CreateDevice", "no D3D11 driver available")))
}

fn create_d3d11_device_with_driver(
    driver: D3D_DRIVER_TYPE,
) -> Result<(ID3D11Device, ID3D11DeviceContext), CaptureError> {
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    let mut feature_level = D3D_FEATURE_LEVEL_11_0;

    // SAFETY: D3D11CreateDevice with null adapter uses the primary GPU.
    // Output pointers are valid stack locals.
    let result = unsafe {
        D3D11CreateDevice(
            None,
            driver,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut feature_level),
            Some(&mut context),
        )
    };
    if let Err(e) = result {
        return Err(init_failed("D3D11CreateDevice", e));
    }
    let device = device.ok_or_else(|| {
        tracing::warn!("D3D11CreateDevice returned null device");
        CaptureError::InitFailed("D3D11CreateDevice: null device".into())
    })?;
    let context = context.ok_or_else(|| {
        tracing::warn!("D3D11CreateDevice returned null context");
        CaptureError::InitFailed("D3D11CreateDevice: null context".into())
    })?;
    Ok((device, context))
}

fn create_winrt_device(device: &ID3D11Device) -> Result<IDirect3DDevice, CaptureError> {
    let dxgi_device: IDXGIDevice = device
        .cast()
        .map_err(|e| init_failed("cast IDXGIDevice", e))?;
    // SAFETY: dxgi_device is valid, obtained from a D3D11 device.
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device) }
        .map_err(|e| init_failed("CreateDirect3D11Device", e))?;
    let winrt_device: IDirect3DDevice = inspectable
        .cast()
        .map_err(|e| init_failed("cast IDirect3DDevice", e))?;
    Ok(winrt_device)
}

/// Creates a `GraphicsCaptureItem` from an `HMONITOR`.
///
/// # Safety
/// `hmonitor` must be a valid monitor handle.
unsafe fn create_capture_item(hmonitor: HMONITOR) -> Result<GraphicsCaptureItem, CaptureError> {
    use windows::Win32::System::WinRT::RoGetActivationFactory;
    let class_id = windows::core::HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureItem");
    let interop: IGraphicsCaptureItemInterop =
        RoGetActivationFactory(&class_id).map_err(|e| init_failed("RoGetActivationFactory", e))?;
    let item = interop
        .CreateForMonitor(hmonitor)
        .map_err(|e| init_failed("CreateForMonitor", e))?;
    Ok(item)
}

/// A frame grabber for a specific monitor.
///
/// Owns a `Direct3D11CaptureFramePool` and `GraphicsCaptureSession`.
pub(crate) struct FrameGrabber {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    device: ID3D11Device,
    context: Arc<Mutex<ID3D11DeviceContext>>,
    closed: bool,
}

// SAFETY: COM/WinRT objects are reference-counted and can be moved to a
// different thread. The session object is not shared, so `Sync` is not
// required.
unsafe impl Send for FrameGrabber {}

impl FrameGrabber {
    /// Creates a new `FrameGrabber` for the given monitor.
    ///
    /// # Safety
    /// `hmonitor` must be a valid `HMONITOR`.
    #[tracing::instrument(skip(d3d, hmonitor), fields(w = width, h = height))]
    pub(crate) unsafe fn new(
        d3d: &D3D11Context,
        hmonitor: HMONITOR,
        width: u32,
        height: u32,
    ) -> Result<Self, CaptureError> {
        ensure_winrt_initialized()?;
        let item = create_capture_item(hmonitor)?;
        let pool = Direct3D11CaptureFramePool::Create(
            &d3d.winrt_device(),
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            SizeInt32 {
                Width: i32::try_from(width).unwrap_or(i32::MAX),
                Height: i32::try_from(height).unwrap_or(i32::MAX),
            },
        )
        .map_err(|e| init_failed("FramePool::Create", e))?;

        let session = pool.CreateCaptureSession(&item).map_err(|e| {
            let _ = pool.Close();
            init_failed("CreateCaptureSession", e)
        })?;
        if let Err(e) = session.StartCapture() {
            let _ = session.Close();
            let _ = pool.Close();
            return Err(init_failed("StartCapture", e));
        }

        tracing::debug!(width, height, "frame grabber created");
        Ok(Self {
            pool,
            session,
            device: d3d.device(),
            context: d3d.context(),
            closed: false,
        })
    }

    /// Attempts to retrieve the next captured frame without blocking.
    ///
    /// Returns `Ok(None)` if no new frame is available.
    pub(crate) fn try_get_next_frame(&self) -> Result<Option<CapturedImage>, CaptureError> {
        ensure_winrt_initialized()?;
        let frame = match self.pool.TryGetNextFrame() {
            Ok(f) => f,
            // The Windows Runtime reports "no new frame" as a successful
            // S_OK HRESULT error result; treat it as `None`.
            Err(e) if e.code().is_ok() => return Ok(None),
            Err(e) => return Err(frame_failed("TryGetNextFrame", e)),
        };
        let image = extract_pixels_from_frame(&frame, &self.device, &self.context)?;
        Ok(Some(image))
    }

    /// Stops the capture session and closes the frame pool.
    pub(crate) fn close(&mut self) -> Result<(), CaptureError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        ensure_winrt_initialized()?;
        let session_result = self.session.Close();
        let pool_result = self.pool.Close();
        match (session_result, pool_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(e), _) | (_, Err(e)) => Err(frame_failed("close capture resources", e)),
        }
    }
}

impl Drop for FrameGrabber {
    fn drop(&mut self) {
        if let Err(e) = self.close() {
            tracing::warn!(error = %e, "failed to close frame grabber during drop");
        }
    }
}

/// Extracts pixel data from a `Direct3D11CaptureFrame`.
///
/// Copies the frame's surface to a staging texture, maps it to CPU
/// memory, and extracts pixel data row-by-row (handling pitch alignment).
fn extract_pixels_from_frame(
    frame: &Direct3D11CaptureFrame,
    device: &ID3D11Device,
    context: &Mutex<ID3D11DeviceContext>,
) -> Result<CapturedImage, CaptureError> {
    let context = context
        .lock()
        .map_err(|_| frame_failed("device context lock", "mutex poisoned"))?;
    let surface: IDirect3DSurface = frame
        .Surface()
        .map_err(|e| frame_failed("frame.Surface", e))?;

    let dxgi_access: IDirect3DDxgiInterfaceAccess = surface
        .cast()
        .map_err(|e| frame_failed("cast IDirect3DDxgiInterfaceAccess", e))?;
    // SAFETY: dxgi_access is a valid IDirect3DDxgiInterfaceAccess from the capture surface.
    let src_texture: ID3D11Texture2D =
        unsafe { dxgi_access.GetInterface() }.map_err(|e| frame_failed("GetInterface", e))?;

    let mut src_desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: src_texture is valid from the capture frame.
    unsafe { src_texture.GetDesc(&mut src_desc) };
    let width = src_desc.Width;
    let height = src_desc.Height;

    let staging_desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: src_desc.Format,
        SampleDesc: src_desc.SampleDesc,
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: 0x20000, // D3D11_CPU_ACCESS_READ
        MiscFlags: 0,
    };

    let mut staging_texture: Option<ID3D11Texture2D> = None;
    // SAFETY: staging_desc is a valid texture description.
    unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging_texture)) }
        .map_err(|e| frame_failed("CreateTexture2D", e))?;
    let staging_texture = staging_texture.ok_or_else(|| {
        tracing::warn!("CreateTexture2D returned null texture");
        CaptureError::FrameGrabFailed("CreateTexture2D returned null".into())
    })?;

    // SAFETY: Both textures are valid with compatible formats.
    unsafe {
        let dst_resource: ID3D11Resource = staging_texture
            .cast()
            .map_err(|e| frame_failed("cast staging texture", e))?;
        let src_resource: ID3D11Resource = src_texture
            .cast()
            .map_err(|e| frame_failed("cast source texture", e))?;
        context.CopyResource(&dst_resource, &src_resource);
    }

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: Staging texture was created with STAGING + CPU_ACCESS_READ.
    let result = unsafe {
        let resource: ID3D11Resource = staging_texture
            .cast()
            .map_err(|e| frame_failed("cast staging texture", e))?;
        context.Map(&resource, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
    };
    if let Err(e) = result {
        return Err(frame_failed("Map", e));
    }

    let row_pitch = mapped.RowPitch as usize;
    let bpp = 4;
    let row_bytes = width as usize * bpp;
    let mut data = Vec::with_capacity(row_bytes * height as usize);

    // SAFETY: mapped.pData is valid for the duration of the Map.
    unsafe {
        let base = mapped.pData as *const u8;
        for row in 0..height as usize {
            let src = base.add(row * row_pitch);
            data.extend_from_slice(std::slice::from_raw_parts(src, row_bytes));
        }
    }

    // SAFETY: Unmap after successful Map is always safe.
    unsafe {
        let resource: ID3D11Resource = staging_texture
            .cast()
            .map_err(|e| frame_failed("cast staging texture", e))?;
        context.Unmap(&resource, 0);
    }

    CapturedImage::new(width, height, PixelFormat::Bgra8, data)
        .map_err(|e| frame_failed("CapturedImage::new", e))
}

/// Crops a captured image to the specified sub-region.
///
/// Returns `None` if the region is entirely outside the image.
pub(crate) fn crop_image(
    image: &CapturedImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Option<CapturedImage> {
    if width == 0 || height == 0 || x >= image.width || y >= image.height {
        return None;
    }
    let end_x = x.saturating_add(width).min(image.width);
    let end_y = y.saturating_add(height).min(image.height);
    let crop_w = end_x - x;
    let crop_h = end_y - y;
    if crop_w == 0 || crop_h == 0 {
        return None;
    }

    let bpp = image.format.bytes_per_pixel();
    let src_row_bytes = image.width as usize * bpp;
    let dst_row_bytes = crop_w as usize * bpp;
    let mut data = Vec::with_capacity(dst_row_bytes * crop_h as usize);

    for row in 0..crop_h as usize {
        let src_offset = (y as usize + row) * src_row_bytes + x as usize * bpp;
        let src_end = src_offset + dst_row_bytes;
        data.extend_from_slice(&image.data[src_offset..src_end]);
    }

    CapturedImage::new(crop_w, crop_h, image.format, data).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtrans_core::types::PixelFormat;

    fn make_image(w: u32, h: u32, fill: u8) -> CapturedImage {
        CapturedImage::new(w, h, PixelFormat::Bgra8, vec![fill; (w * h * 4) as usize]).unwrap()
    }

    #[test]
    fn crop_full_image() {
        let img = make_image(100, 100, 42);
        let cropped = crop_image(&img, 0, 0, 100, 100).unwrap();
        assert_eq!(cropped.width, 100);
        assert_eq!(cropped.height, 100);
        assert_eq!(cropped.data.len(), 100 * 100 * 4);
    }

    #[test]
    fn crop_partial() {
        let img = make_image(100, 100, 42);
        let cropped = crop_image(&img, 10, 20, 30, 40).unwrap();
        assert_eq!(cropped.width, 30);
        assert_eq!(cropped.height, 40);
        assert_eq!(cropped.data.len(), 30 * 40 * 4);
    }

    #[test]
    fn crop_zero_width() {
        let img = make_image(100, 100, 42);
        assert!(crop_image(&img, 0, 0, 0, 100).is_none());
    }

    #[test]
    fn crop_out_of_bounds() {
        let img = make_image(100, 100, 42);
        assert!(crop_image(&img, 200, 0, 10, 10).is_none());
    }

    #[test]
    fn crop_clips_to_bounds() {
        let img = make_image(100, 100, 42);
        let cropped = crop_image(&img, 90, 90, 50, 50).unwrap();
        assert_eq!(cropped.width, 10);
        assert_eq!(cropped.height, 10);
    }

    #[test]
    fn crop_preserves_pixel_values() {
        let mut data = vec![0u8; 8 * 8 * 4];
        let idx = (2 * 8 + 3) * 4;
        data[idx] = 0;
        data[idx + 1] = 0;
        data[idx + 2] = 255;
        data[idx + 3] = 255;
        let img = CapturedImage::new(8, 8, PixelFormat::Bgra8, data).unwrap();
        let cropped = crop_image(&img, 2, 1, 4, 4).unwrap();
        assert_eq!(cropped.width, 4);
        assert_eq!(cropped.height, 4);
        let cidx = (4 + 1) * 4;
        assert_eq!(cropped.data[cidx + 2], 255);
        assert_eq!(cropped.data[cidx + 3], 255);
    }
}
