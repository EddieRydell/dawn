#![cfg_attr(not(windows), allow(dead_code))]

use dawn_app_core::output_runtime::OutputFrame;
use serde::{Deserialize, Serialize};
use specta::Type;

const FRAME_HEADER_LEN: usize = 64;
const FRAME_SLOT_COUNT: usize = 3;
const FRAME_MAGIC: u32 = 0x4441_4652;
const FRAME_VERSION: u32 = 1;
const FRAME_OFFSET_PIXEL_COUNT: usize = 8;
const FRAME_OFFSET_PAYLOAD_BYTES: usize = 12;
const FRAME_OFFSET_LATEST_SEQ: usize = 16;
const FRAME_OFFSET_LATEST_SLOT: usize = 20;
const FRAME_OFFSET_CURRENT_TIME: usize = 24;
const FRAME_OFFSET_PLAYING: usize = 32;
const FRAME_OFFSET_BACKEND_MS: usize = 36;
const FRAME_OFFSET_SLOT_COUNT: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PreviewTransportMode {
    Webview2Shared,
    Unsupported,
}

#[derive(Default)]
pub struct PreviewTransportRuntime {
    seq: u32,
    platform: PlatformRuntime,
}

impl PreviewTransportRuntime {
    pub fn init_window(
        &mut self,
        window: &tauri::WebviewWindow,
        pixel_count: usize,
    ) -> Result<(), String> {
        self.platform.init_window(window, pixel_count)
    }

    pub fn dispose_window(&mut self, label: &str) {
        self.platform.dispose_window(label);
    }

    pub fn has_sinks(&self) -> bool {
        self.platform.has_sinks()
    }

    pub fn publish_frame(&mut self, frame: &OutputFrame, playing: bool, backend_ms: f32) {
        if !self.has_sinks() {
            return;
        }
        self.seq = self.seq.wrapping_add(1).max(1);
        self.platform
            .publish_frame(self.seq, frame, playing, backend_ms);
    }

    pub fn mode() -> PreviewTransportMode {
        PlatformRuntime::mode()
    }
}

fn frame_payload_bytes(pixel_count: usize) -> Option<usize> {
    pixel_count.checked_mul(3)
}

fn write_slice(bytes: &mut [u8], offset: usize, src: &[u8]) -> bool {
    let Some(end) = offset.checked_add(src.len()) else {
        return false;
    };
    let Some(dst) = bytes.get_mut(offset..end) else {
        return false;
    };
    dst.copy_from_slice(src);
    true
}

fn write_frame_rgb(frame: &OutputFrame, pixel_count: usize, dest: &mut [u8]) -> usize {
    let mut written_pixels = 0usize;
    for fixture in &frame.fixtures {
        for pixel in &fixture.pixels {
            if written_pixels >= pixel_count {
                return written_pixels * 3;
            }
            let offset = written_pixels * 3;
            let Some(dst) = dest.get_mut(offset..offset + 3) else {
                return offset;
            };
            dst[0] = pixel.color.red;
            dst[1] = pixel.color.green;
            dst[2] = pixel.color.blue;
            written_pixels += 1;
        }
    }
    written_pixels * 3
}

#[cfg(not(windows))]
#[derive(Default)]
struct PlatformRuntime;

#[cfg(not(windows))]
impl PlatformRuntime {
    fn init_window(
        &mut self,
        _window: &tauri::WebviewWindow,
        _pixel_count: usize,
    ) -> Result<(), String> {
        Err("preview shared buffers are only available on Windows".to_string())
    }

    fn dispose_window(&mut self, _label: &str) {}

    fn has_sinks(&self) -> bool {
        false
    }

    fn publish_frame(&mut self, _seq: u32, _frame: &OutputFrame, _playing: bool, _backend_ms: f32) {
    }

    fn mode() -> PreviewTransportMode {
        PreviewTransportMode::Unsupported
    }
}

#[cfg(windows)]
mod windows_platform {
    use std::collections::HashMap;
    use std::sync::mpsc;

    use tauri::WebviewWindow;
    use windows_core::Interface;

    use super::{
        frame_payload_bytes, write_frame_rgb, write_slice, OutputFrame, PreviewTransportMode,
        FRAME_HEADER_LEN, FRAME_MAGIC, FRAME_OFFSET_BACKEND_MS, FRAME_OFFSET_CURRENT_TIME,
        FRAME_OFFSET_LATEST_SEQ, FRAME_OFFSET_LATEST_SLOT, FRAME_OFFSET_PAYLOAD_BYTES,
        FRAME_OFFSET_PIXEL_COUNT, FRAME_OFFSET_PLAYING, FRAME_OFFSET_SLOT_COUNT, FRAME_SLOT_COUNT,
        FRAME_VERSION,
    };

    #[derive(Default)]
    pub(super) struct PlatformRuntime {
        sinks: HashMap<String, FrameSink>,
    }

    struct FrameSink {
        #[allow(dead_code)]
        shared: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2SharedBuffer,
        ptr: *mut u8,
        pixel_count: usize,
    }

    // SAFETY: WebView2 owns the mapped shared-buffer memory while `shared` is
    // retained. Dawn serializes access through `PreviewTransportRuntime`.
    unsafe impl Send for FrameSink {}
    // SAFETY: Shared access is guarded by the runtime mutex in `AppState`.
    unsafe impl Sync for FrameSink {}

    impl PlatformRuntime {
        pub(super) fn init_window(
            &mut self,
            window: &WebviewWindow,
            pixel_count: usize,
        ) -> Result<(), String> {
            let sink = create_frame_sink_for_window(window, pixel_count)?;
            self.sinks.insert(window.label().to_string(), sink);
            Ok(())
        }

        pub(super) fn dispose_window(&mut self, label: &str) {
            self.sinks.remove(label);
        }

        pub(super) fn has_sinks(&self) -> bool {
            !self.sinks.is_empty()
        }

        pub(super) fn publish_frame(
            &mut self,
            seq: u32,
            frame: &OutputFrame,
            playing: bool,
            backend_ms: f32,
        ) {
            for sink in self.sinks.values_mut() {
                let Some(payload_bytes) = frame_payload_bytes(sink.pixel_count) else {
                    continue;
                };
                let total_len = FRAME_HEADER_LEN + payload_bytes * FRAME_SLOT_COUNT;
                // SAFETY: The pointer is a live WebView2 shared buffer mapping
                // retained by `sink.shared`; writes are serialized by caller.
                let bytes = unsafe { std::slice::from_raw_parts_mut(sink.ptr, total_len) };
                let slot = (seq as usize) % FRAME_SLOT_COUNT;
                let slot_offset = FRAME_HEADER_LEN + slot * payload_bytes;
                let Some(slot_slice) = bytes.get_mut(slot_offset..slot_offset + payload_bytes)
                else {
                    continue;
                };
                let written = write_frame_rgb(frame, sink.pixel_count, slot_slice);
                if let Some(remainder) = slot_slice.get_mut(written..) {
                    remainder.fill(0);
                }
                let Some(slot_u32) = u32::try_from(slot).ok() else {
                    continue;
                };
                let _ = write_slice(
                    bytes,
                    FRAME_OFFSET_CURRENT_TIME,
                    &(frame.time_ms as f64).to_le_bytes(),
                );
                let _ = write_slice(bytes, FRAME_OFFSET_PLAYING, &[u8::from(playing)]);
                let _ = write_slice(bytes, FRAME_OFFSET_BACKEND_MS, &backend_ms.to_le_bytes());
                let _ = write_slice(bytes, FRAME_OFFSET_LATEST_SLOT, &slot_u32.to_le_bytes());
                let _ = write_slice(bytes, FRAME_OFFSET_LATEST_SEQ, &seq.to_le_bytes());
            }
        }

        pub(super) fn mode() -> PreviewTransportMode {
            PreviewTransportMode::Webview2Shared
        }
    }

    fn create_frame_sink_for_window(
        window: &WebviewWindow,
        pixel_count: usize,
    ) -> Result<FrameSink, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        window
            .with_webview(move |webview| {
                // SAFETY: Tauri provides a live platform WebView2 handle inside
                // this closure; the returned COM buffer is retained by caller.
                let _ = tx.send(unsafe { create_frame_sink(&webview, pixel_count) });
            })
            .map_err(|error| format!("with_webview(frame) failed: {error}"))?;
        rx.recv()
            .map_err(|error| format!("failed to receive preview frame buffer: {error}"))?
    }

    unsafe fn create_frame_sink(
        webview: &tauri::webview::PlatformWebview,
        pixel_count: usize,
    ) -> Result<FrameSink, String> {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment12;

        let Some(payload_bytes) = frame_payload_bytes(pixel_count) else {
            return Err("preview pixel count exceeds shared buffer format".to_string());
        };
        let total_len = FRAME_HEADER_LEN + payload_bytes * FRAME_SLOT_COUNT;
        let env: ICoreWebView2Environment12 = webview
            .environment()
            .cast()
            .map_err(|error| format!("WebView2 environment cast failed: {error}"))?;
        let shared = env
            .CreateSharedBuffer(u64::try_from(total_len).unwrap_or(u64::MAX))
            .map_err(|error| format!("CreateSharedBuffer(frame) failed: {error}"))?;
        let mut ptr = std::ptr::null_mut();
        shared
            .Buffer(std::ptr::addr_of_mut!(ptr))
            .map_err(|error| format!("Shared frame buffer mapping failed: {error}"))?;
        let bytes = std::slice::from_raw_parts_mut(ptr, total_len);
        bytes.fill(0);
        let pixel_count = u32::try_from(pixel_count)
            .map_err(|_| "preview pixel count exceeds shared buffer format".to_string())?;
        let payload_bytes = u32::try_from(payload_bytes)
            .map_err(|_| "preview payload exceeds shared buffer format".to_string())?;
        let slot_count = u32::try_from(FRAME_SLOT_COUNT)
            .map_err(|_| "preview slot count exceeds shared buffer format".to_string())?;
        let _ = write_slice(bytes, 0, &FRAME_MAGIC.to_le_bytes());
        let _ = write_slice(bytes, 4, &FRAME_VERSION.to_le_bytes());
        let _ = write_slice(bytes, FRAME_OFFSET_PIXEL_COUNT, &pixel_count.to_le_bytes());
        let _ = write_slice(
            bytes,
            FRAME_OFFSET_PAYLOAD_BYTES,
            &payload_bytes.to_le_bytes(),
        );
        let _ = write_slice(bytes, FRAME_OFFSET_SLOT_COUNT, &slot_count.to_le_bytes());
        post_shared_buffer(webview, &shared)?;
        Ok(FrameSink {
            shared,
            ptr,
            pixel_count: pixel_count as usize,
        })
    }

    unsafe fn post_shared_buffer(
        webview: &tauri::webview::PlatformWebview,
        shared: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2SharedBuffer,
    ) -> Result<(), String> {
        use webview2_com::CoTaskMemPWSTR;
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_17;

        let webview17: ICoreWebView2_17 = webview
            .controller()
            .CoreWebView2()
            .map_err(|error| format!("CoreWebView2 lookup failed: {error}"))?
            .cast()
            .map_err(|error| format!("WebView2 cast to ICoreWebView2_17 failed: {error}"))?;
        let additional_data = CoTaskMemPWSTR::from(r#"{"kind":"frame"}"#);
        webview17
            .PostSharedBufferToScript(
                shared,
                webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
                *additional_data.as_ref().as_pcwstr(),
            )
            .map_err(|error| format!("PostSharedBufferToScript failed: {error}"))?;
        Ok(())
    }
}

#[cfg(windows)]
use windows_platform::PlatformRuntime;
