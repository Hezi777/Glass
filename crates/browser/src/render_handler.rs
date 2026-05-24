//! CEF Render Handler
//!
//! Implements CEF's RenderHandler trait to capture off-screen rendered frames.
//! When shared_texture_enabled is set, CEF calls on_accelerated_paint with an
//! IOSurface handle. We wrap it as a CVPixelBuffer for zero-copy rendering
//! through GPUI's Surface element.

use crate::events::{BrowserEvent, EventSender};
use cef::{
    AcceleratedPaintInfo, Browser, ImplRenderHandler, PaintElementType, Rect, RenderHandler,
    ScreenInfo, WrapRenderHandler, rc::Rc as _, wrap_render_handler,
};
#[cfg(target_os = "macos")]
use core_foundation::base::TCFType;
#[cfg(target_os = "macos")]
use core_video::pixel_buffer::CVPixelBuffer;
#[cfg(target_os = "macos")]
#[allow(deprecated)]
use io_surface::IOSurface;
use parking_lot::Mutex;
use std::sync::Arc;

pub struct RenderState {
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    #[cfg(target_os = "macos")]
    pub current_frame: Option<CVPixelBuffer>,
    #[cfg(not(target_os = "macos"))]
    pub current_frame: Option<Arc<(Vec<u8>, u32, u32)>>,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            scale_factor: 1.0,
            #[cfg(target_os = "macos")]
            current_frame: None,
            #[cfg(not(target_os = "macos"))]
            current_frame: None,
        }
    }
}

#[derive(Clone)]
pub struct OsrRenderHandler {
    state: Arc<Mutex<RenderState>>,
    sender: EventSender,
}

impl OsrRenderHandler {
    pub fn new(state: Arc<Mutex<RenderState>>, sender: EventSender) -> Self {
        Self { state, sender }
    }
}

wrap_render_handler! {
    pub struct RenderHandlerBuilder {
        handler: OsrRenderHandler,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                let state = self.handler.state.lock();
                rect.x = 0;
                rect.y = 0;
                rect.width = state.width as i32;
                rect.height = state.height as i32;
            }
        }

        fn screen_info(
            &self,
            _browser: Option<&mut Browser>,
            screen_info: Option<&mut ScreenInfo>,
        ) -> ::std::os::raw::c_int {
            if let Some(info) = screen_info {
                let state = self.handler.state.lock();
                info.device_scale_factor = state.scale_factor;
                info.rect.x = 0;
                info.rect.y = 0;
                info.rect.width = state.width as i32;
                info.rect.height = state.height as i32;
                info.available_rect = info.rect.clone();
                info.depth = 32;
                info.depth_per_component = 8;
                info.is_monochrome = 0;
                return 1;
            }
            0
        }

        fn screen_point(
            &self,
            _browser: Option<&mut Browser>,
            view_x: ::std::os::raw::c_int,
            view_y: ::std::os::raw::c_int,
            screen_x: Option<&mut ::std::os::raw::c_int>,
            screen_y: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            if let Some(screen_x) = screen_x {
                *screen_x = view_x;
            }
            if let Some(screen_y) = screen_y {
                *screen_y = view_y;
            }
            1
        }

        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            if type_ != PaintElementType::default() {
                return;
            }

            #[cfg(target_os = "macos")]
            {
                let Some(info) = info else {
                    log::warn!("[browser::render_handler] on_accelerated_paint() no info");
                    return;
                };

                let io_surface_ptr = info.shared_texture_io_surface;
                if io_surface_ptr.is_null() {
                    log::warn!("[browser::render_handler] on_accelerated_paint() null IOSurface");
                    return;
                }

                #[allow(deprecated)]
                let io_surface: IOSurface = unsafe {
                    TCFType::wrap_under_get_rule(io_surface_ptr as io_surface::IOSurfaceRef)
                };

                let pixel_buffer = match CVPixelBuffer::from_io_surface(&io_surface, None) {
                    Ok(pb) => pb,
                    Err(err) => {
                        log::error!("[browser::render_handler] on_accelerated_paint() CVPixelBuffer::from_io_surface failed: {:?}", err);
                        return;
                    }
                };

                self.handler.state.lock().current_frame = Some(pixel_buffer);
                let _ = self.handler.sender.send(BrowserEvent::FrameReady);
            }

            #[cfg(not(target_os = "macos"))]
            {
                let _ = info;
            }
        }

        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ) {
            if type_ != PaintElementType::default() {
                return;
            }

            #[cfg(not(target_os = "macos"))]
            {
                if buffer.is_null() {
                    log::warn!("[browser::render_handler] on_paint() null buffer");
                    return;
                }

                let Ok(width) = u32::try_from(width) else {
                    log::warn!("[browser::render_handler] on_paint() invalid width");
                    return;
                };
                let Ok(height) = u32::try_from(height) else {
                    log::warn!("[browser::render_handler] on_paint() invalid height");
                    return;
                };
                if width == 0 || height == 0 {
                    log::warn!("[browser::render_handler] on_paint() empty frame");
                    return;
                }
                let Some(len) = width
                    .checked_mul(height)
                    .and_then(|pixels| pixels.checked_mul(4))
                    .map(|len| len as usize)
                else {
                    log::warn!("[browser::render_handler] on_paint() frame too large");
                    return;
                };

                let mut bytes = unsafe { std::slice::from_raw_parts(buffer, len).to_vec() };
                for pixel in bytes.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
                let mut state = self.handler.state.lock();
                state.current_frame = Some(Arc::new((bytes, width, height)));
                let _ = self.handler.sender.send(BrowserEvent::FrameReady);
            }

            #[cfg(target_os = "macos")]
            {
                let _ = (buffer, width, height);
                log::warn!("[browser::render_handler] on_paint() called unexpectedly (shared_texture_enabled should prevent this)");
            }
        }
    }
}

impl RenderHandlerBuilder {
    pub fn build(handler: OsrRenderHandler) -> cef::RenderHandler {
        Self::new(handler)
    }
}
