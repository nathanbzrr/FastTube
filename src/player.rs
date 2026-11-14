use crate::settings::BufferSettings;
use eframe::egui::{self, Rect};
use eframe::egui_glow;
use eframe::glow;
use eframe::glow::HasContext;
use libloading::Library;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SW_HIDE, SW_SHOWNOACTIVATE, SWP_HIDEWINDOW, SWP_NOACTIVATE,
    SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowPos, ShowWindow, WS_CHILD, WS_VISIBLE,
};

type Create = unsafe extern "C" fn() -> *mut c_void;
type SetOption = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type Initialize = unsafe extern "C" fn(*mut c_void) -> c_int;
type Command = unsafe extern "C" fn(*mut c_void, *const *const c_char) -> c_int;
type Terminate = unsafe extern "C" fn(*mut c_void);
type GetProperty = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char;
type SetProperty = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type Free = unsafe extern "C" fn(*mut c_void);
type WaitEvent = unsafe extern "C" fn(*mut c_void, f64) -> *const MpvEvent;
type RequestEvent = unsafe extern "C" fn(*mut c_void, c_int, u64) -> c_int;
type ErrorString = unsafe extern "C" fn(c_int) -> *const c_char;
type RenderContextCreate =
    unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *mut MpvRenderParam) -> c_int;
type RenderContextFree = unsafe extern "C" fn(*mut c_void);
type RenderContextRender = unsafe extern "C" fn(*mut c_void, *mut MpvRenderParam) -> c_int;
type RenderContextUpdate = unsafe extern "C" fn(*mut c_void) -> u64;
type RenderContextSetUpdateCallback =
    unsafe extern "C" fn(*mut c_void, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void);
type GetProcAddress = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;

const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const MPV_RENDER_PARAM_OPENGL_FBO: c_int = 3;
const MPV_RENDER_PARAM_FLIP_Y: c_int = 4;
const MPV_RENDER_API_TYPE_OPENGL: &[u8] = b"opengl\0";

#[repr(C)]
struct MpvEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *const c_void,
}

#[repr(C)]
struct StartFile {
    playlist_entry_id: u64,
}

#[repr(C)]
struct EndFile {
    reason: c_int,
    error: c_int,
    playlist_entry_id: u64,
    _playlist_insert_id: u64,
}

#[repr(C)]
struct MpvRenderParam {
    type_: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct MpvOpenglInitParams {
    get_proc_address: Option<GetProcAddress>,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct MpvOpenglFbo {
    fbo: c_int,
    width: c_int,
    height: c_int,
    internal_format: c_int,
}

pub(crate) fn scaled_window_bounds(rect: Rect, scale: f32) -> (i32, i32, i32, i32) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    (
        (rect.min.x * scale).round() as i32,
        (rect.min.y * scale).round() as i32,
        (rect.width() * scale).round().max(1.0) as i32,
        (rect.height() * scale).round().max(1.0) as i32,
    )
}

pub(crate) struct VideoWindow {
    hwnd: HWND,
    visible: bool,
    last_rect: Option<(Rect, f32)>,
}

impl VideoWindow {
    pub(crate) fn new(parent: HWND) -> Result<Self, String> {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                ptr::null(),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                1,
                1,
                parent,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err("Não foi possível criar a área de vídeo.".into());
        }
        Ok(Self {
            hwnd,
            visible: true,
            last_rect: None,
        })
    }

    pub(crate) fn hwnd(&self) -> HWND {
        self.hwnd
    }

    #[cfg(test)]
    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    pub(crate) fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        unsafe {
            ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
        if visible && let Some((rect, scale)) = self.last_rect {
            self.apply_bounds(rect, scale);
        }
    }

    pub(crate) fn resize(&mut self, rect: Rect, scale: f32) {
        if !rect.is_positive() {
            return;
        }
        self.last_rect = Some((rect, scale));
        if self.visible {
            self.apply_bounds(rect, scale);
        }
    }

    fn apply_bounds(&self, rect: Rect, scale: f32) {
        let (x, y, width, height) = scaled_window_bounds(rect, scale);
        let visibility_flag = if self.visible {
            SWP_SHOWWINDOW
        } else {
            SWP_HIDEWINDOW
        };
        unsafe {
            SetWindowPos(
                self.hwnd,
                ptr::null_mut(),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER | visibility_flag,
            );
        }
    }
}

impl Drop for VideoWindow {
    fn drop(&mut self) {
        unsafe {
            ShowWindow(self.hwnd, SW_HIDE);
            DestroyWindow(self.hwnd);
        }
    }
}

struct MpvApi {
    create: Create,
    set_option: SetOption,
    initialize: Initialize,
    command: Command,
    terminate: Terminate,
    get_property: GetProperty,
    set_property: SetProperty,
    free: Free,
    wait_event: WaitEvent,
    request_event: RequestEvent,
    error_string: ErrorString,
    render_create: RenderContextCreate,
    render_free: RenderContextFree,
    render_render: RenderContextRender,
    render_update: RenderContextUpdate,
    render_set_update_callback: RenderContextSetUpdateCallback,
}

impl MpvApi {
    unsafe fn load(library: &Library) -> Result<Self, String> {
        unsafe {
            Ok(Self {
                create: *library
                    .get(b"mpv_create\0")
                    .map_err(|error| error.to_string())?,
                set_option: *library
                    .get(b"mpv_set_option_string\0")
                    .map_err(|error| error.to_string())?,
                initialize: *library
                    .get(b"mpv_initialize\0")
                    .map_err(|error| error.to_string())?,
                command: *library
                    .get(b"mpv_command\0")
                    .map_err(|error| error.to_string())?,
                terminate: *library
                    .get(b"mpv_terminate_destroy\0")
                    .map_err(|error| error.to_string())?,
                get_property: *library
                    .get(b"mpv_get_property_string\0")
                    .map_err(|error| error.to_string())?,
                set_property: *library
                    .get(b"mpv_set_property_string\0")
                    .map_err(|error| error.to_string())?,
                free: *library
                    .get(b"mpv_free\0")
                    .map_err(|error| error.to_string())?,
                wait_event: *library
                    .get(b"mpv_wait_event\0")
                    .map_err(|error| error.to_string())?,
                request_event: *library
                    .get(b"mpv_request_event\0")
                    .map_err(|error| error.to_string())?,
                error_string: *library
                    .get(b"mpv_error_string\0")
                    .map_err(|error| error.to_string())?,
                render_create: *library
                    .get(b"mpv_render_context_create\0")
                    .map_err(|error| error.to_string())?,
                render_free: *library
                    .get(b"mpv_render_context_free\0")
                    .map_err(|error| error.to_string())?,
                render_render: *library
                    .get(b"mpv_render_context_render\0")
                    .map_err(|error| error.to_string())?,
                render_update: *library
                    .get(b"mpv_render_context_update\0")
                    .map_err(|error| error.to_string())?,
                render_set_update_callback: *library
                    .get(b"mpv_render_context_set_update_callback\0")
                    .map_err(|error| error.to_string())?,
            })
        }
    }

    fn describe_error(&self, code: c_int) -> String {
        let text = unsafe { (self.error_string)(code) };
        if text.is_null() {
            return format!("Erro do player ({code})");
        }
        format!(
            "Erro do player: {}",
            unsafe { CStr::from_ptr(text) }.to_string_lossy()
        )
    }
}

static OPENGL32: OnceLock<Option<Library>> = OnceLock::new();
static EGL: OnceLock<Option<Library>> = OnceLock::new();

fn opengl32() -> Option<&'static Library> {
    OPENGL32
        .get_or_init(|| unsafe { Library::new("opengl32.dll").ok() })
        .as_ref()
}

fn egl() -> Option<&'static Library> {
    EGL.get_or_init(|| unsafe {
        Library::new("libEGL.dll")
            .or_else(|_| Library::new("EGL.dll"))
            .ok()
    })
    .as_ref()
}

unsafe extern "C" fn mpv_get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    if name.is_null() {
        return ptr::null_mut();
    }
    let bytes = unsafe { CStr::from_ptr(name) }.to_bytes_with_nul();
    if let Some(library) = opengl32() {
        if let Ok(wgl_get_proc_address) = unsafe {
            library.get::<unsafe extern "system" fn(*const c_char) -> *mut c_void>(
                b"wglGetProcAddress\0",
            )
        } {
            let address = unsafe { wgl_get_proc_address(name) };
            let special = address as isize;
            if !address.is_null() && !(1..=3).contains(&special) && special != -1 {
                return address;
            }
        }
        if let Ok(symbol) = unsafe { library.get::<unsafe extern "system" fn()>(bytes) } {
            return *symbol as usize as *mut c_void;
        }
    }
    if let Some(library) = egl()
        && let Ok(egl_get_proc_address) = unsafe {
            library.get::<unsafe extern "system" fn(*const c_char) -> *mut c_void>(
                b"eglGetProcAddress\0",
            )
        }
    {
        let address = unsafe { egl_get_proc_address(name) };
        if !address.is_null() {
            return address;
        }
    }
    ptr::null_mut()
}

unsafe extern "C" fn mpv_render_update_callback(data: *mut c_void) {
    if data.is_null() {
        return;
    }
    let ctx = unsafe { &*(data as *const egui::Context) };
    ctx.request_repaint();
}

struct RenderContext {
    handle: *mut c_void,
    render: RenderContextRender,
    update: RenderContextUpdate,
    set_update_callback: RenderContextSetUpdateCallback,
    free: RenderContextFree,
    repaint_ctx: Box<egui::Context>,
}

unsafe impl Send for RenderContext {}
unsafe impl Sync for RenderContext {}

impl Drop for RenderContext {
    fn drop(&mut self) {
        unsafe {
            (self.set_update_callback)(self.handle, None, ptr::null_mut());
            (self.free)(self.handle);
        }
        let _ = &self.repaint_ctx;
    }
}

struct GlSurface {
    texture: glow::Texture,
    framebuffer: glow::Framebuffer,
    width: i32,
    height: i32,
}

struct RenderShared {
    gl: Arc<glow::Context>,
    context: RenderContext,
    surface: Mutex<Option<GlSurface>>,
    needs_render: AtomicBool,
    failed: AtomicBool,
}

unsafe impl Send for RenderShared {}
unsafe impl Sync for RenderShared {}

impl RenderShared {
    fn render(&self, info: egui::PaintCallbackInfo, painter: &egui_glow::Painter) {
        if self.failed.load(Ordering::Relaxed) {
            return;
        }
        let update_flags = unsafe { (self.context.update)(self.context.handle) };
        if (update_flags & 1) != 0 {
            self.needs_render.store(true, Ordering::Relaxed);
        }
        let clip = info.clip_rect_in_pixels();
        if clip.width_px <= 0 || clip.height_px <= 0 {
            return;
        }
        let ppp = info.pixels_per_point.max(0.5);
        let screen_h = info.screen_size_px[1] as i32;
        let dst_left = (info.viewport.min.x * ppp).round() as i32;
        let dst_top = (info.viewport.min.y * ppp).round() as i32;
        let width = (info.viewport.width() * ppp).round() as i32;
        let height = (info.viewport.height() * ppp).round() as i32;
        if width <= 0 || height <= 0 {
            return;
        }
        let dst_bottom = screen_h - (dst_top + height);
        let vis_x0 = dst_left.max(clip.left_px);
        let vis_y0 = dst_bottom.max(clip.from_bottom_px);
        let vis_x1 = (dst_left + width).min(clip.left_px + clip.width_px);
        let vis_y1 = (dst_bottom + height).min(clip.from_bottom_px + clip.height_px);
        if vis_x1 <= vis_x0 || vis_y1 <= vis_y0 {
            return;
        }
        let gl = self.gl.as_ref();
        let mut surface = match self.surface.lock() {
            Ok(surface) => surface,
            Err(_) => return,
        };
        let recreate = !matches!(
            surface.as_ref(),
            Some(existing) if existing.width == width && existing.height == height
        );
        if recreate {
            let Some(created) = create_surface(gl, width, height) else {
                self.failed.store(true, Ordering::Relaxed);
                return;
            };
            if let Some(previous) = surface.replace(created) {
                destroy_surface(gl, previous);
            }
        }
        let Some(surface) = surface.as_ref() else {
            return;
        };
        let should_render_mpv = recreate || self.needs_render.swap(false, Ordering::Relaxed);
        unsafe {
            if should_render_mpv {
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(surface.framebuffer));
                gl.viewport(0, 0, width, height);
                gl.disable(glow::SCISSOR_TEST);
                let mut fbo = MpvOpenglFbo {
                    fbo: surface.framebuffer.0.get() as c_int,
                    width,
                    height,
                    internal_format: 0,
                };
                let mut flip_y: c_int = 1;
                let mut params = [
                    MpvRenderParam {
                        type_: MPV_RENDER_PARAM_OPENGL_FBO,
                        data: (&mut fbo as *mut MpvOpenglFbo).cast(),
                    },
                    MpvRenderParam {
                        type_: MPV_RENDER_PARAM_FLIP_Y,
                        data: (&mut flip_y as *mut c_int).cast(),
                    },
                    MpvRenderParam {
                        type_: 0,
                        data: ptr::null_mut(),
                    },
                ];
                let result = (self.context.render)(self.context.handle, params.as_mut_ptr());
                if result < 0 {
                    self.failed.store(true, Ordering::Relaxed);
                    gl.bind_framebuffer(glow::FRAMEBUFFER, painter.intermediate_fbo());
                    return;
                }
            }
            let src_x0 = vis_x0 - dst_left;
            let src_y0 = vis_y0 - dst_bottom;
            let src_x1 = vis_x1 - dst_left;
            let src_y1 = vis_y1 - dst_bottom;
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(surface.framebuffer));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, painter.intermediate_fbo());
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(
                clip.left_px,
                clip.from_bottom_px,
                clip.width_px,
                clip.height_px,
            );
            gl.blit_framebuffer(
                src_x0,
                src_y0,
                src_x1,
                src_y1,
                vis_x0,
                vis_y0,
                vis_x1,
                vis_y1,
                glow::COLOR_BUFFER_BIT,
                glow::NEAREST,
            );
            gl.disable(glow::SCISSOR_TEST);
            gl.bind_framebuffer(glow::FRAMEBUFFER, painter.intermediate_fbo());
        }
    }
}

impl Drop for RenderShared {
    fn drop(&mut self) {
        if let Ok(mut surface) = self.surface.lock()
            && let Some(surface) = surface.take()
        {
            destroy_surface(self.gl.as_ref(), surface);
        }
    }
}

fn create_surface(gl: &glow::Context, width: i32, height: i32) -> Option<GlSurface> {
    unsafe {
        let texture = gl.create_texture().ok()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            width,
            height,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );
        gl.bind_texture(glow::TEXTURE_2D, None);
        let framebuffer = match gl.create_framebuffer() {
            Ok(framebuffer) => framebuffer,
            Err(_) => {
                gl.delete_texture(texture);
                return None;
            }
        };
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        if status != glow::FRAMEBUFFER_COMPLETE {
            gl.delete_framebuffer(framebuffer);
            gl.delete_texture(texture);
            return None;
        }
        Some(GlSurface {
            texture,
            framebuffer,
            width,
            height,
        })
    }
}

fn destroy_surface(gl: &glow::Context, surface: GlSurface) {
    unsafe {
        gl.delete_framebuffer(surface.framebuffer);
        gl.delete_texture(surface.texture);
    }
}

pub struct Player {
    handle: *mut c_void,
    window: Option<VideoWindow>,
    render: Option<Arc<RenderShared>>,
    command: Command,
    terminate: Terminate,
    get_property: GetProperty,
    set_property: SetProperty,
    free: Free,
    wait_event: WaitEvent,
    error_string: ErrorString,
    _library: Library,
}

impl Player {
    pub fn new(
        frame: &eframe::Frame,
        buffer: BufferSettings,
        ctx: &egui::Context,
    ) -> Result<Self, String> {
        let parent = match frame
            .window_handle()
            .map_err(|error| error.to_string())?
            .as_raw()
        {
            RawWindowHandle::Win32(handle) => handle.hwnd.get() as HWND,
            _ => return Err("A janela do Windows não está disponível.".into()),
        };
        let mut render_error = None;
        if let Some(gl) = frame.gl() {
            match Self::new_render_api(buffer, ctx, Arc::clone(gl)) {
                Ok(player) => return Ok(player),
                Err(error) => render_error = Some(error),
            }
        }
        Self::new_embedded(parent, buffer).map_err(|error| match render_error {
            Some(render_error) => {
                format!("{error} (a renderização OpenGL do libmpv também falhou: {render_error})")
            }
            None => error,
        })
    }

    fn load_library() -> Result<Library, String> {
        for name in ["libmpv-2.dll", "mpv-2.dll"] {
            let mut candidates = Vec::new();
            if let Ok(executable) = std::env::current_exe() {
                let mut current = executable.parent();
                for _ in 0..4 {
                    let Some(directory) = current else { break };
                    candidates.push(directory.join(name));
                    current = directory.parent();
                }
            }
            if let Ok(directory) = std::env::current_dir() {
                candidates.push(directory.join(name));
            }
            candidates.push(name.into());
            for candidate in candidates {
                if let Ok(found) = unsafe { Library::new(&candidate) } {
                    return Ok(found);
                }
            }
        }
        Err("Não foi possível carregar libmpv-2.dll ou mpv-2.dll. Verifique se a DLL e suas dependências estão na pasta do aplicativo ou no PATH.".into())
    }

    fn new_render_api(
        buffer: BufferSettings,
        ctx: &egui::Context,
        gl: Arc<glow::Context>,
    ) -> Result<Self, String> {
        let library = Self::load_library()?;
        unsafe {
            let api = MpvApi::load(&library)?;
            let handle = (api.create)();
            if handle.is_null() {
                return Err("Não foi possível iniciar o libmpv.".into());
            }
            let mut player = Self {
                handle,
                window: None,
                render: None,
                command: api.command,
                terminate: api.terminate,
                get_property: api.get_property,
                set_property: api.set_property,
                free: api.free,
                wait_event: api.wait_event,
                error_string: api.error_string,
                _library: library,
            };
            let options = Self::options(buffer, None, true);
            for (name, value) in options {
                let option_name = CString::new(name).unwrap();
                let value = CString::new(value).unwrap();
                let result = (api.set_option)(handle, option_name.as_ptr(), value.as_ptr());
                if result < 0 {
                    let detail = player.describe_error(result);
                    return Err(format!("Não foi possível configurar {name}: {detail}"));
                }
            }
            let result = (api.initialize)(handle);
            if result >= 0 {
                for event_id in [6, 7, 8] {
                    let event_result = (api.request_event)(handle, event_id, 0);
                    if event_result < 0 {
                        let detail = player.describe_error(event_result);
                        return Err(format!(
                            "N??o foi poss??vel habilitar a correla????o de eventos do libmpv: {detail}"
                        ));
                    }
                }
            }
            if result < 0 {
                let detail = player.describe_error(result);
                return Err(format!("Não foi possível inicializar o libmpv: {detail}"));
            }
            let render_context = create_render_context(&api, handle, ctx)?;
            player.render = Some(Arc::new(RenderShared {
                gl,
                context: render_context,
                surface: Mutex::new(None),
                needs_render: AtomicBool::new(true),
                failed: AtomicBool::new(false),
            }));
            Ok(player)
        }
    }

    fn new_embedded(parent: HWND, buffer: BufferSettings) -> Result<Self, String> {
        let library = Self::load_library()?;
        unsafe {
            let api = MpvApi::load(&library)?;
            let window = VideoWindow::new(parent)?;
            let wid = window.hwnd() as isize;
            let handle = (api.create)();
            if handle.is_null() {
                return Err("Não foi possível iniciar o libmpv.".into());
            }
            let mut player = Self {
                handle,
                window: Some(window),
                render: None,
                command: api.command,
                terminate: api.terminate,
                get_property: api.get_property,
                set_property: api.set_property,
                free: api.free,
                wait_event: api.wait_event,
                error_string: api.error_string,
                _library: library,
            };
            let options = Self::options(buffer, Some(wid), false);
            for (name, value) in options {
                let option_name = CString::new(name).unwrap();
                let value = CString::new(value).unwrap();
                let result = (api.set_option)(handle, option_name.as_ptr(), value.as_ptr());
                if result < 0 {
                    let detail = player.describe_error(result);
                    (api.terminate)(handle);
                    return Err(format!("Não foi possível configurar {name}: {detail}"));
                }
            }
            let result = (api.initialize)(handle);
            if result >= 0 {
                for event_id in [6, 7, 8] {
                    let event_result = (api.request_event)(handle, event_id, 0);
                    if event_result < 0 {
                        let detail = player.describe_error(event_result);
                        (api.terminate)(handle);
                        return Err(format!(
                            "N??o foi poss??vel habilitar a correla????o de eventos do libmpv: {detail}"
                        ));
                    }
                }
            }
            if result < 0 {
                let detail = player.describe_error(result);
                (api.terminate)(handle);
                return Err(format!("Não foi possível inicializar o libmpv: {detail}"));
            }
            player.resize(Rect::NOTHING, 1.0);
            Ok(player)
        }
    }

    fn options(
        buffer: BufferSettings,
        wid: Option<isize>,
        render_api: bool,
    ) -> Vec<(&'static str, String)> {
        let mut options = Vec::new();
        if let Some(wid) = wid {
            options.push(("wid", format!("{wid}")));
        }
        if render_api {
            options.push(("vo", "libmpv".to_owned()));
        }
        options.extend([
            ("input-default-bindings", "no".to_owned()),
            ("input-vo-keyboard", "no".to_owned()),
            ("osc", "no".to_owned()),
            ("terminal", "no".to_owned()),
            ("hwdec", "auto-safe".to_owned()),
            ("demuxer-max-bytes", format!("{}MiB", buffer.max_mib)),
            ("demuxer-max-back-bytes", format!("{}MiB", buffer.back_mib)),
            ("cache-secs", format!("{}", buffer.cache_seconds)),
            (
                "ytdl-format",
                "bestvideo[height<=1080]+bestaudio/best[height<=1080]/best".to_owned(),
            ),
        ]);
        if let Ok(executable) = std::env::current_exe()
            && let Some(yt_dlp) = crate::search::bundled_yt_dlp(&executable)
        {
            options.push(("script-opts", crate::search::ytdl_hook_path_option(&yt_dlp)));
        }
        options
    }

    pub fn play(&mut self, url: &str) -> Result<(), String> {
        self.run(&["loadfile", url, "replace"])
    }

    pub fn add_subtitle(
        &self,
        path_or_url: &str,
        title: &str,
        language: &str,
    ) -> Result<(), String> {
        if let Some(existing_id) = self.find_subtitle_track_by_path(path_or_url) {
            self.set("sid", &existing_id)?;
        } else {
            self.run(&["sub-add", path_or_url, "select", title, language])?;
        }
        self.set("sub-visibility", "yes")
    }

    pub fn disable_subtitles(&self) -> Result<(), String> {
        self.set("sid", "no")
    }

    fn find_subtitle_track_by_path(&self, path_or_url: &str) -> Option<String> {
        let count = self.number("track-list/count")? as usize;
        let target = std::path::Path::new(path_or_url);
        for index in 0..count {
            if self
                .property(&format!("track-list/{index}/type"))
                .as_deref()
                != Some("sub")
            {
                continue;
            }
            let Some(external) = self.property(&format!("track-list/{index}/external-filename"))
            else {
                continue;
            };
            if external == path_or_url || std::path::Path::new(&external) == target {
                return self.property(&format!("track-list/{index}/id"));
            }
        }
        None
    }

    fn run(&self, args: &[&str]) -> Result<(), String> {
        let strings: Vec<CString> = args
            .iter()
            .map(|arg| CString::new(*arg))
            .collect::<Result<_, _>>()
            .map_err(|error| error.to_string())?;
        let mut pointers: Vec<*const c_char> = strings.iter().map(|arg| arg.as_ptr()).collect();
        pointers.push(ptr::null());
        let result = unsafe { (self.command)(self.handle, pointers.as_ptr()) };
        if result < 0 {
            Err(self.describe_error(result))
        } else {
            Ok(())
        }
    }

    fn describe_error(&self, code: c_int) -> String {
        let text = unsafe { (self.error_string)(code) };
        if text.is_null() {
            return format!("Erro do player ({code})");
        }
        format!(
            "Erro do player: {}",
            unsafe { CStr::from_ptr(text) }.to_string_lossy()
        )
    }

    pub fn property(&self, name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        let value = unsafe { (self.get_property)(self.handle, name.as_ptr()) };
        if value.is_null() {
            return None;
        }
        let text = unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned();
        unsafe {
            (self.free)(value.cast());
        }
        Some(text)
    }

    pub fn number(&self, name: &str) -> Option<f64> {
        self.property(name)?
            .parse()
            .ok()
            .filter(|value: &f64| value.is_finite())
    }

    pub fn set(&self, name: &str, value: &str) -> Result<(), String> {
        let name = CString::new(name).map_err(|error| error.to_string())?;
        let value = CString::new(value).map_err(|error| error.to_string())?;
        let result = unsafe { (self.set_property)(self.handle, name.as_ptr(), value.as_ptr()) };
        if result < 0 {
            Err(self.describe_error(result))
        } else {
            Ok(())
        }
    }

    pub fn seek(&self, seconds: f64, mode: &str) -> Result<(), String> {
        if !seconds.is_finite() {
            return Err("A posição de reprodução deve ser um número finito.".into());
        }
        self.run(&["seek", &format!("{seconds:.3}"), mode])
    }

    pub fn quality(
        &self,
        url: &str,
        height: Option<u32>,
        default_format: &str,
    ) -> Result<(), String> {
        let position = self.number("time-pos");
        let format = match height {
            Some(height) => format!(
                "bestvideo[height={height}]+bestaudio/best[height={height}]/bestvideo[height={height}]"
            ),
            None => default_format.to_owned(),
        };
        let previous = self.property("options/ytdl-format");
        self.set("options/ytdl-format", &format)?;
        let result = if let Some(position) = position {
            self.run(&[
                "loadfile",
                url,
                "replace",
                "-1",
                &format!("start={position:.3}"),
            ])
        } else {
            self.run(&["loadfile", url, "replace"])
        };
        if result.is_err()
            && let Some(previous) = previous
        {
            let _ = self.set("options/ytdl-format", &previous);
        }
        result
    }

    pub fn poll_events(&self) -> (Vec<crate::queue::PlaybackEvent>, Option<String>) {
        let mut events = Vec::new();
        let mut error = None;
        while let Some(event) = unsafe { (self.wait_event)(self.handle, 0.0).as_ref() } {
            match event.event_id {
                0 => break,
                6 if !event.data.is_null() => {
                    let start = unsafe { &*(event.data as *const StartFile) };
                    events.push(crate::queue::PlaybackEvent::StartFile {
                        playlist_entry_id: start.playlist_entry_id,
                    });
                }
                7 if !event.data.is_null() => {
                    let end = unsafe { &*(event.data as *const EndFile) };
                    let failed = end.reason == 4;
                    if failed && error.is_none() {
                        error = Some(self.describe_error(end.error));
                    }
                    events.push(crate::queue::PlaybackEvent::EndFile {
                        playlist_entry_id: end.playlist_entry_id,
                        failed,
                    });
                }
                8 => events.push(crate::queue::PlaybackEvent::FileLoaded),
                _ => {}
            }
        }
        (events, error)
    }

    pub fn set_visible(&mut self, visible: bool) {
        if let Some(window) = &mut self.window {
            window.set_visible(visible);
        }
    }

    pub fn resize(&mut self, rect: Rect, scale: f32) {
        if let Some(window) = &mut self.window {
            window.resize(rect, scale);
        }
    }

    pub fn is_render_api(&self) -> bool {
        self.render
            .as_ref()
            .is_some_and(|shared| !shared.failed.load(Ordering::Relaxed))
    }

    /// Returns `false` in native `HWND` fallback mode where `egui` cannot draw over video.
    pub fn paint_into(&self, painter: &egui::Painter, rect: Rect) -> bool {
        let Some(shared) = &self.render else {
            return false;
        };
        if shared.failed.load(Ordering::Relaxed) {
            return false;
        }
        let shared = Arc::downgrade(shared);
        let callback = egui_glow::CallbackFn::new(move |info, painter| {
            if let Some(shared) = shared.upgrade() {
                shared.render(info, painter);
            }
        });
        painter.add(egui::PaintCallback {
            rect,
            callback: Arc::new(callback),
        });
        true
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Some(window) = &mut self.window {
            window.set_visible(false);
        }
        self.render = None;
        unsafe {
            (self.terminate)(self.handle);
        }
    }
}

unsafe fn create_render_context(
    api: &MpvApi,
    handle: *mut c_void,
    ctx: &egui::Context,
) -> Result<RenderContext, String> {
    unsafe {
        let mut init = MpvOpenglInitParams {
            get_proc_address: Some(mpv_get_proc_address),
            get_proc_address_ctx: ptr::null_mut(),
        };
        let mut params = [
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_API_TYPE,
                data: MPV_RENDER_API_TYPE_OPENGL.as_ptr().cast_mut().cast(),
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: (&mut init as *mut MpvOpenglInitParams).cast(),
            },
            MpvRenderParam {
                type_: 0,
                data: ptr::null_mut(),
            },
        ];
        let mut render_handle: *mut c_void = ptr::null_mut();
        let result = (api.render_create)(&mut render_handle, handle, params.as_mut_ptr());
        if result < 0 || render_handle.is_null() {
            return Err(api.describe_error(result));
        }
        let repaint_ctx = Box::new(ctx.clone());
        (api.render_set_update_callback)(
            render_handle,
            Some(mpv_render_update_callback),
            (&*repaint_ctx as *const egui::Context).cast_mut().cast(),
        );
        Ok(RenderContext {
            handle: render_handle,
            render: api.render_render,
            update: api.render_update,
            set_update_callback: api.render_set_update_callback,
            free: api.render_free,
            repaint_ctx,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BufferProfile;
    use eframe::egui::Pos2;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GWL_STYLE, GetClientRect, GetWindowLongW};

    #[test]
    fn video_window_hides_and_restores_hwnd_preserving_latest_bounds() {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let parent = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                ptr::null(),
                0,
                0,
                0,
                800,
                600,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        assert!(!parent.is_null());

        let mut video_window = VideoWindow::new(parent).expect("child window should be created");
        assert!(video_window.is_visible());
        let style_initial = unsafe { GetWindowLongW(video_window.hwnd(), GWL_STYLE) as u32 };
        assert_ne!(style_initial & WS_VISIBLE, 0);

        let initial_rect = Rect::from_min_max(Pos2::new(16.0, 48.0), Pos2::new(656.0, 408.0));
        video_window.resize(initial_rect, 1.0);

        video_window.set_visible(false);
        assert!(!video_window.is_visible());
        let style_hidden = unsafe { GetWindowLongW(video_window.hwnd(), GWL_STYLE) as u32 };
        assert_eq!(style_hidden & WS_VISIBLE, 0);

        // Bounds changed while hidden must still apply once the HWND is restored.
        let fullscreen_rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1280.0, 720.0));
        video_window.resize(fullscreen_rect, 1.5);
        let style_still_hidden = unsafe { GetWindowLongW(video_window.hwnd(), GWL_STYLE) as u32 };
        assert_eq!(style_still_hidden & WS_VISIBLE, 0);

        video_window.set_visible(true);
        assert!(video_window.is_visible());
        let style_restored = unsafe { GetWindowLongW(video_window.hwnd(), GWL_STYLE) as u32 };
        assert_ne!(style_restored & WS_VISIBLE, 0);

        let mut client = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let ok = unsafe { GetClientRect(video_window.hwnd(), &mut client) };
        assert_ne!(ok, 0);
        assert_eq!(client.right - client.left, 1920);
        assert_eq!(client.bottom - client.top, 1080);

        drop(video_window);
        unsafe {
            DestroyWindow(parent);
        }
    }

    #[test]
    fn render_api_options_use_libmpv_vo_and_embedded_options_use_wid() {
        let buffer = BufferProfile::Default.values();
        let render = Player::options(buffer, None, true);
        assert!(
            render
                .iter()
                .any(|(name, value)| *name == "vo" && value == "libmpv")
        );
        assert!(!render.iter().any(|(name, _)| *name == "wid"));

        let embedded = Player::options(buffer, Some(42), false);
        assert!(
            embedded
                .iter()
                .any(|(name, value)| *name == "wid" && value == "42")
        );
        assert!(!embedded.iter().any(|(name, _)| *name == "vo"));
    }
}
