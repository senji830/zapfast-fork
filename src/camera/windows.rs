//! Native Windows camera capture via Media Foundation, and screen capture via GDI.

use super::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::*;

/// Discovers cameras available on Windows via Media Foundation.
pub fn cameras() -> Vec<(String, String)> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).is_err() {
            CoUninitialize();
            return Vec::new();
        }

        let mut attrs: Option<IMFAttributes> = None;
        if MFCreateAttributes(&mut attrs, 1).is_err() {
            let _ = MFShutdown();
            CoUninitialize();
            return Vec::new();
        }
        let Some(attrs) = attrs else {
            let _ = MFShutdown();
            CoUninitialize();
            return Vec::new();
        };

        if attrs
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .is_err()
        {
            let _ = MFShutdown();
            CoUninitialize();
            return Vec::new();
        }

        let mut devices_ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count: u32 = 0;
        if MFEnumDeviceSources(&attrs, &mut devices_ptr, &mut count).is_err()
            || devices_ptr.is_null()
        {
            let _ = MFShutdown();
            CoUninitialize();
            return Vec::new();
        }

        let mut devices = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let ptr = devices_ptr.add(i);
            if let Some(activate) = std::ptr::read(ptr) {
                devices.push(activate);
            }
        }
        CoTaskMemFree(Some(devices_ptr.cast()));

        let mut list = Vec::new();
        for activate in devices {
            let name = get_attribute_string(&activate, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)
                .unwrap_or_else(|| "Camera".to_owned());
            let id = get_attribute_string(
                &activate,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
            )
            .unwrap_or_else(|| name.clone());
            list.push((id, name));
        }

        let _ = MFShutdown();
        CoUninitialize();
        list
    }
}

/// Helper to read allocated wide string from an attribute store.
unsafe fn get_attribute_string(attrs: &IMFAttributes, guid: &GUID) -> Option<String> {
    let mut pwstr = PWSTR(std::ptr::null_mut());
    let mut len = 0u32;
    if attrs.GetAllocatedString(guid, &mut pwstr, &mut len).is_ok() && !pwstr.0.is_null() {
        let slice = std::slice::from_raw_parts(pwstr.0, len as usize);
        let s = String::from_utf16(slice).ok();
        CoTaskMemFree(Some(pwstr.0.cast()));
        s
    } else {
        None
    }
}

/// Whether a device string represents a usable camera on Windows.
pub fn is_capture(device: &str) -> bool {
    if device.is_empty() {
        return false;
    }
    let list = cameras();
    if list.is_empty() {
        return device.starts_with(r"\\?\") || !device.is_empty();
    }
    list.iter().any(|(id, name)| id == device || name == device)
}

/// The current or default capture frame size.
pub fn current_size(_device: &str) -> Option<(u32, u32)> {
    Some((1280, 720))
}

/// The largest frame size supported.
pub fn largest_size(_device: &str, _fps: u32) -> Option<(u32, u32)> {
    Some((1280, 720))
}

/// Media Foundation camera capture source.
pub struct MfSource {
    reader: IMFSourceReader,
    media_source: IMFMediaSource,
    size: (usize, usize),
    is_yuy2: bool,
    stopped: Arc<AtomicBool>,
}

impl MfSource {
    /// Opens the specified camera (or default) using Media Foundation.
    pub fn open(device: &str, budget: (usize, usize), _fps: u32) -> Result<Self, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET)
                .map_err(|e| format!("MFStartup failed: {e}"))?;

            let mut attrs: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attrs, 1)
                .map_err(|e| format!("MFCreateAttributes failed: {e}"))?;
            let attrs = attrs.ok_or_else(|| "no attributes".to_owned())?;
            attrs
                .SetGUID(
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
                )
                .map_err(|e| format!("SetGUID failed: {e}"))?;

            let mut devices_ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut count: u32 = 0;
            MFEnumDeviceSources(&attrs, &mut devices_ptr, &mut count)
                .map_err(|e| format!("MFEnumDeviceSources failed: {e}"))?;
            if count == 0 || devices_ptr.is_null() {
                let _ = MFShutdown();
                CoUninitialize();
                return Err("no video capture devices found".to_owned());
            }

            let mut devices = Vec::with_capacity(count as usize);
            for i in 0..count as usize {
                let ptr = devices_ptr.add(i);
                if let Some(activate) = std::ptr::read(ptr) {
                    devices.push(activate);
                }
            }
            CoTaskMemFree(Some(devices_ptr.cast()));

            let mut chosen_activate: Option<IMFActivate> = None;
            for activate in devices {
                let id = get_attribute_string(
                    &activate,
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                );
                let name = get_attribute_string(&activate, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME);
                if device.is_empty()
                    || id.as_deref() == Some(device)
                    || name.as_deref() == Some(device)
                {
                    chosen_activate = Some(activate);
                    break;
                }
                if chosen_activate.is_none() {
                    chosen_activate = Some(activate);
                }
            }

            let chosen_activate =
                chosen_activate.ok_or_else(|| format!("camera {device} not found"))?;

            let media_source = MFCreateDeviceSource(&chosen_activate)
                .map_err(|e| format!("MFCreateDeviceSource failed: {e}"))?;

            let reader = MFCreateSourceReaderFromMediaSource(&media_source, None)
                .map_err(|e| format!("MFCreateSourceReaderFromMediaSource failed: {e}"))?;

            let mut is_yuy2 = true;
            let mut size = budget;
            let mut set_success = false;

            for (subtype, is_yuy) in [
                (&MFVideoFormat_YUY2, true),
                (&MFVideoFormat_NV12, false),
            ] {
                if let Ok(mt) = MFCreateMediaType() {
                    let _ = mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video);
                    let _ = mt.SetGUID(&MF_MT_SUBTYPE, subtype);
                    let packed: u64 = ((budget.0 as u64) << 32) | (budget.1 as u64);
                    let _ = mt.SetUINT64(&MF_MT_FRAME_SIZE, packed);
                    if reader
                        .SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM, None, &mt)
                        .is_ok()
                    {
                        is_yuy2 = is_yuy;
                        set_success = true;
                        break;
                    }
                }
            }

            if !set_success {
                if let Ok(ct) = reader.GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM) {
                    if let Ok(packed) = ct.GetUINT64(&MF_MT_FRAME_SIZE) {
                        let w = (packed >> 32) as usize;
                        let h = (packed & 0xFFFF_FFFF) as usize;
                        if w > 0 && h > 0 {
                            size = (w, h);
                        }
                    }
                    if let Ok(sub) = ct.GetGUID(&MF_MT_SUBTYPE) {
                        is_yuy2 = sub == MFVideoFormat_YUY2;
                    }
                }
            }

            Ok(Self {
                reader,
                media_source,
                size,
                is_yuy2,
                stopped: Arc::new(AtomicBool::new(false)),
            })
        }
    }

    /// Reads one video frame from Media Foundation into `out`.
    pub fn read(&mut self, out: &mut [u8]) -> Read {
        if self.stopped.load(Ordering::Relaxed) {
            return Read::Ended;
        }
        unsafe {
            let mut stream_flags: u32 = 0;
            let mut sample: Option<IMFSample> = None;
            let res = self.reader.ReadSample(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM,
                0,
                None,
                Some(&mut stream_flags),
                None,
                Some(&mut sample),
            );
            if res.is_err() {
                return Read::Ended;
            }
            if (stream_flags & MF_SOURCE_READERF_ENDOFSTREAM.0) != 0 {
                return Read::Ended;
            }
            let Some(sample) = sample else {
                return Read::Idle;
            };
            let Ok(buffer) = sample.ConvertToContiguousBuffer() else {
                return Read::Idle;
            };
            let mut ptr: *mut u8 = std::ptr::null_mut();
            let mut current_len: u32 = 0;
            if buffer.Lock(&mut ptr, None, Some(&mut current_len)).is_err() || ptr.is_null() {
                return Read::Idle;
            }
            let slice = std::slice::from_raw_parts(ptr, current_len as usize);
            let (width, height) = self.size;
            if self.is_yuy2 {
                crate::camera::to_yuv420(slice, width * 2, width, height, out);
            } else {
                crate::camera::to_yuv420_from_nv12(slice, width, height, out);
            }
            let _ = buffer.Unlock();
            Read::Frame
        }
    }

    /// Stops camera capture and shuts down Media Foundation objects.
    pub fn stop(&mut self) {
        if !self.stopped.swap(true, Ordering::Relaxed) {
            unsafe {
                let _ = self.media_source.Shutdown();
                let _ = MFShutdown();
                CoUninitialize();
            }
        }
    }
}

impl Drop for MfSource {
    fn drop(&mut self) {
        self.stop();
    }
}

/// GDI screen capture source for desktop screen sharing on Windows.
pub struct GdiScreenSource {
    hdc_screen: HDC,
    hdc_mem: HDC,
    hbitmap: HBITMAP,
    size: (usize, usize),
    screen_size: (i32, i32),
    raw_buffer: Vec<u8>,
    stopped: Arc<AtomicBool>,
}

impl GdiScreenSource {
    /// Opens the desktop display for screen capture.
    pub fn open(budget: (usize, usize), _fps: u32) -> Result<Self, String> {
        unsafe {
            let hdc_screen = GetDC(HWND::default());
            if hdc_screen.0.is_null() {
                return Err("GetDC failed".to_owned());
            }
            let screen_w = GetDeviceCaps(hdc_screen, HORZRES);
            let screen_h = GetDeviceCaps(hdc_screen, VERTRES);
            let hdc_mem = CreateCompatibleDC(hdc_screen);
            if hdc_mem.0.is_null() {
                ReleaseDC(HWND::default(), hdc_screen);
                return Err("CreateCompatibleDC failed".to_owned());
            }
            let (w, h) = budget;
            let hbitmap = CreateCompatibleBitmap(hdc_screen, w as i32, h as i32);
            if hbitmap.0.is_null() {
                DeleteDC(hdc_mem);
                ReleaseDC(HWND::default(), hdc_screen);
                return Err("CreateCompatibleBitmap failed".to_owned());
            }
            SelectObject(hdc_mem, hbitmap);
            let raw_buffer = vec![0u8; w * h * 4];
            Ok(Self {
                hdc_screen,
                hdc_mem,
                hbitmap,
                size: budget,
                screen_size: (screen_w, screen_h),
                raw_buffer,
                stopped: Arc::new(AtomicBool::new(false)),
            })
        }
    }

    /// Reads one desktop screen frame converted into YUV420.
    pub fn read(&mut self, out: &mut [u8]) -> Read {
        if self.stopped.load(Ordering::Relaxed) {
            return Read::Ended;
        }
        unsafe {
            SetStretchBltMode(self.hdc_mem, COLORONCOLOR);
            let _ = StretchBlt(
                self.hdc_mem,
                0,
                0,
                self.size.0 as i32,
                self.size.1 as i32,
                self.hdc_screen,
                0,
                0,
                self.screen_size.0,
                self.screen_size.1,
                SRCCOPY,
            );
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: self.size.0 as i32,
                    biHeight: -(self.size.1 as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            GetDIBits(
                self.hdc_mem,
                self.hbitmap,
                0,
                self.size.1 as u32,
                Some(self.raw_buffer.as_mut_ptr().cast()),
                &mut bmi,
                DIB_RGB_COLORS,
            );
            crate::camera::to_yuv420_from_rgb(
                &self.raw_buffer,
                self.size.0,
                self.size.1,
                self.size.0 * 4,
                true,
                out,
            );
            Read::Frame
        }
    }

    /// Stops screen capture and releases GDI handles.
    pub fn stop(&mut self) {
        if !self.stopped.swap(true, Ordering::Relaxed) {
            unsafe {
                if !self.hbitmap.0.is_null() {
                    let _ = DeleteObject(self.hbitmap);
                }
                if !self.hdc_mem.0.is_null() {
                    let _ = DeleteDC(self.hdc_mem);
                }
                if !self.hdc_screen.0.is_null() {
                    ReleaseDC(HWND::default(), self.hdc_screen);
                }
            }
        }
    }
}

impl Drop for GdiScreenSource {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Unified Windows video source (webcam or screen).
pub enum Source {
    Mf(MfSource),
    Gdi(GdiScreenSource),
}

impl Source {
    /// Opens the camera source.
    pub fn open(device: &str, budget: (usize, usize), fps: u32) -> Result<Self, String> {
        MfSource::open(device, budget, fps).map(Source::Mf)
    }

    /// Opens the screen capture source.
    pub fn open_screen(budget: (usize, usize), fps: u32) -> Result<Self, String> {
        GdiScreenSource::open(budget, fps).map(Source::Gdi)
    }

    /// The frame dimensions delivered by this source.
    pub fn size(&self) -> (usize, usize) {
        match self {
            Source::Mf(mf) => mf.size,
            Source::Gdi(gdi) => gdi.size,
        }
    }

    /// Reads one whole frame into `out`.
    pub fn read(&mut self, out: &mut [u8]) -> Read {
        match self {
            Source::Mf(mf) => mf.read(out),
            Source::Gdi(gdi) => gdi.read(out),
        }
    }

    /// Stops capture.
    pub fn stop(&mut self) {
        match self {
            Source::Mf(mf) => mf.stop(),
            Source::Gdi(gdi) => gdi.stop(),
        }
    }
}
