//! Native Windows 11 desktop integration: DWM attributes (dark titlebar,
//! rounded corners, Mica backdrop), Snap Layouts (`WM_NCHITTEST` `HTMAXBUTTON`)
//! & Taskbar Thumbnail Toolbar (`WM_COMMAND` `THBN_CLICKED`) subclassing,
//! zero-polling Windows theme registry observer, `ITaskbarList3` progress &
//! thumbnail toolbar, `ICustomDestinationList` Jump Lists, native Windows Toast
//! notifications, and portable HKCU `fasttube://` protocol registration.

use eframe::egui;
use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

#[cfg(windows)]
use std::collections::HashMap;
#[cfg(windows)]
use std::ptr;
#[cfg(windows)]
use std::sync::{Mutex, OnceLock};
#[cfg(windows)]
use std::time::{Duration, Instant};

/// Explicit AppUserModelID shared across Jump Lists, Taskbar, and Toasts.
pub const FASTTUBE_APP_USER_MODEL_ID: &str = "FastTube.Desktop";

#[cfg(windows)]
const TOAST_MIN_INTERVAL: Duration = Duration::from_secs(2);
const MAX_TOAST_TITLE_CHARS: usize = 128;
const MAX_TOAST_BODY_CHARS: usize = 256;

#[allow(dead_code)]
const THUMB_BTN_PREV_ID: u32 = 1001;
#[allow(dead_code)]
const THUMB_BTN_PLAY_PAUSE_ID: u32 = 1002;
#[allow(dead_code)]
const THUMB_BTN_NEXT_ID: u32 = 1003;

#[allow(dead_code)]
const FASTTUBE_SUBCLASS_ID: usize = 0x4654_3131; // "FT11"

// Win32 / DWM / Taskbar / Registry constants
#[allow(dead_code)]
const DWMWA_USE_IMMERSIVE_DARK_MODE_BEFORE_20H1: u32 = 19;
#[allow(dead_code)]
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
#[allow(dead_code)]
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
#[allow(dead_code)]
const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
#[allow(dead_code)]
const DWMWCP_ROUND: u32 = 2;
#[allow(dead_code)]
const DWMSBT_NONE: u32 = 1;
#[allow(dead_code)]
const DWMSBT_MAINWINDOW: u32 = 2;

#[allow(dead_code)]
const WM_NCDESTROY: u32 = 0x0082;
#[allow(dead_code)]
const WM_NCHITTEST: u32 = 0x0084;
#[allow(dead_code)]
const WM_NCLBUTTONDOWN: u32 = 0x00A1;
#[allow(dead_code)]
const WM_NCLBUTTONUP: u32 = 0x00A2;
#[allow(dead_code)]
const WM_COMMAND: u32 = 0x0111;
#[allow(dead_code)]
const HTMAXBUTTON: isize = 9;
#[allow(dead_code)]
const SW_MAXIMIZE: i32 = 3;
#[allow(dead_code)]
const SW_RESTORE: i32 = 9;
#[allow(dead_code)]
const THBN_CLICKED: u16 = 0x1800;

#[allow(dead_code)]
const TBPF_NOPROGRESS: u32 = 0x0000_0000;
#[allow(dead_code)]
const TBPF_INDETERMINATE: u32 = 0x0000_0001;
#[allow(dead_code)]
const TBPF_NORMAL: u32 = 0x0000_0002;
#[allow(dead_code)]
const TBPF_ERROR: u32 = 0x0000_0004;
#[allow(dead_code)]
const TBPF_PAUSED: u32 = 0x0000_0008;

#[allow(dead_code)]
const THB_ICON: u32 = 0x0000_0002;
#[allow(dead_code)]
const THB_TOOLTIP: u32 = 0x0000_0004;
#[allow(dead_code)]
const THB_FLAGS: u32 = 0x0000_0008;

#[allow(dead_code)]
const THBF_ENABLED: u32 = 0x0000_0000;
#[allow(dead_code)]
const THBF_DISABLED: u32 = 0x0000_0001;
#[allow(dead_code)]
const THBF_HIDDEN: u32 = 0x0000_0008;

#[allow(dead_code)]
const COINIT_APARTMENTTHREADED: u32 = 0x2;
#[allow(dead_code)]
const COINIT_DISABLE_OLE1DDE: u32 = 0x4;
#[allow(dead_code)]
const CLSCTX_INPROC_SERVER: u32 = 0x1;
#[allow(dead_code)]
const S_OK: i32 = 0;
#[allow(dead_code)]
const S_FALSE: i32 = 1;
#[allow(dead_code)]
const RPC_E_CHANGED_MODE: i32 = -2_147_417_850; // 0x8001_0106
#[allow(dead_code)]
const VT_LPWSTR: u16 = 31;

#[allow(dead_code)]
const ERROR_SUCCESS: u32 = 0;
#[allow(dead_code)]
const ERROR_FILE_NOT_FOUND: u32 = 2;
#[allow(dead_code)]
const RRF_RT_REG_SZ: u32 = 0x0000_0002;
#[allow(dead_code)]
const RRF_RT_REG_EXPAND_SZ: u32 = 0x0000_0004;
#[allow(dead_code)]
const RRF_RT_REG_DWORD: u32 = 0x0000_0010;
#[allow(dead_code)]
const REG_SZ: u32 = 1;
#[allow(dead_code)]
const KEY_QUERY_VALUE: u32 = 0x0001;
#[allow(dead_code)]
const KEY_SET_VALUE: u32 = 0x0002;
#[allow(dead_code)]
const KEY_NOTIFY: u32 = 0x0010;
#[allow(dead_code)]
const KEY_WRITE: u32 = 0x0002_0006;
#[allow(dead_code)]
const REG_NOTIFY_CHANGE_LAST_SET: u32 = 0x0000_0004;
#[allow(dead_code)]
const WAIT_OBJECT_0: u32 = 0x0000_0000;
#[allow(dead_code)]
const INFINITE: u32 = 0xFFFF_FFFF;
#[allow(dead_code)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[allow(dead_code, clippy::upper_case_acronyms)]
type HWND = *mut c_void;
#[allow(dead_code, clippy::upper_case_acronyms)]
type HKEY = *mut c_void;
#[allow(dead_code, clippy::upper_case_acronyms)]
type HICON = *mut c_void;
#[allow(dead_code, clippy::upper_case_acronyms)]
type HBITMAP = *mut c_void;
#[allow(dead_code, clippy::upper_case_acronyms)]
type HANDLE = *mut c_void;
#[allow(dead_code, clippy::upper_case_acronyms)]
type HRESULT = i32;

/// Matches Win32 `((HKEY)(ULONG_PTR)((LONG)0x80000001))` on both 32-bit and 64-bit Windows.
#[cfg(windows)]
const HKEY_CURRENT_USER: HKEY = (-2_147_483_647_i32 as isize) as HKEY;

// ---------------------------------------------------------------------------
// COM ABI definitions (`#[repr(C)]`, compiled on all targets for layout tests)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

#[allow(dead_code)]
static CLSID_TASKBAR_LIST: Guid = Guid {
    data1: 0x56FD_F344,
    data2: 0xFD6D,
    data3: 0x11D0,
    data4: [0x95, 0x8A, 0x00, 0x60, 0x97, 0xC9, 0xA0, 0x90],
};

#[allow(dead_code)]
static IID_ITASKBAR_LIST3: Guid = Guid {
    data1: 0xEA1A_FB91,
    data2: 0x9E28,
    data3: 0x4B86,
    data4: [0x90, 0xE9, 0x9E, 0x9F, 0x8A, 0x5E, 0xEF, 0xAF],
};

#[allow(dead_code)]
static CLSID_DESTINATION_LIST: Guid = Guid {
    data1: 0x77F1_0CF0,
    data2: 0x3DB5,
    data3: 0x4966,
    data4: [0xB5, 0x20, 0xB7, 0xC5, 0x4F, 0xD3, 0x5E, 0xD6],
};

#[allow(dead_code)]
static IID_ICUSTOM_DESTINATION_LIST: Guid = Guid {
    data1: 0x6332_DEBF,
    data2: 0x87B5,
    data3: 0x4670,
    data4: [0x90, 0xC0, 0x5E, 0x57, 0xB4, 0x08, 0xA4, 0x9E],
};

#[allow(dead_code)]
static CLSID_ENUMERABLE_OBJECT_COLLECTION: Guid = Guid {
    data1: 0x2D34_68C1,
    data2: 0x36A7,
    data3: 0x43B6,
    data4: [0xAC, 0x24, 0xD3, 0xF0, 0x2F, 0xD9, 0x60, 0x7A],
};

#[allow(dead_code)]
static IID_IOBJECT_ARRAY: Guid = Guid {
    data1: 0x92CA_9DCD,
    data2: 0x5622,
    data3: 0x4BBA,
    data4: [0xA8, 0x05, 0x5E, 0x9F, 0x54, 0x1B, 0xD8, 0xC9],
};

#[allow(dead_code)]
static IID_IOBJECT_COLLECTION: Guid = Guid {
    data1: 0x5632_B1A4,
    data2: 0xE38A,
    data3: 0x400A,
    data4: [0x92, 0x8A, 0xD4, 0xCD, 0x63, 0x23, 0x02, 0x95],
};

#[allow(dead_code)]
static CLSID_SHELL_LINK: Guid = Guid {
    data1: 0x0002_1401,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

#[allow(dead_code)]
static IID_ISHELL_LINK_W: Guid = Guid {
    data1: 0x0002_14F9,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

#[allow(dead_code)]
static IID_IPROPERTY_STORE: Guid = Guid {
    data1: 0x886D_8EEB,
    data2: 0x8CF2,
    data3: 0x4446,
    data4: [0x8D, 0x02, 0xCD, 0xBA, 0x1D, 0xBD, 0xCF, 0x99],
};

#[allow(dead_code)]
static PKEY_TITLE: PropertyKey = PropertyKey {
    fmtid: Guid {
        data1: 0xF29F_85E0,
        data2: 0x4FF9,
        data3: 0x1068,
        data4: [0xAB, 0x91, 0x08, 0x00, 0x2B, 0x27, 0xB3, 0xD9],
    },
    pid: 2,
};

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code, clippy::upper_case_acronyms)]
pub(crate) struct THUMBBUTTON {
    pub dw_mask: u32,
    pub i_id: u32,
    pub i_bitmap: u32,
    pub h_icon: HICON,
    pub sz_tip: [u16; 260],
    pub dw_flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct PropertyKey {
    pub fmtid: Guid,
    pub pid: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct PropVariant {
    pub vt: u16,
    pub w_reserved1: u16,
    pub w_reserved2: u16,
    pub w_reserved3: u16,
    pub data: [*mut c_void; 2],
}

#[repr(C)]
#[allow(dead_code)]
pub(crate) struct IUnknownVtbl {
    pub query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// Exact 21-slot COM vtable for `ITaskbarList3` (`IUnknown` + `ITaskbarList` +
/// `ITaskbarList2` + `ITaskbarList3`).
#[repr(C)]
#[allow(dead_code)]
pub(crate) struct ITaskbarList3Vtbl {
    // IUnknown (0..2)
    pub query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    // ITaskbarList (3..7)
    pub hr_init: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    pub add_tab: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    pub delete_tab: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    pub activate_tab: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    pub set_active_alt: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    // ITaskbarList2 (8)
    pub mark_fullscreen_window: unsafe extern "system" fn(*mut c_void, HWND, i32) -> HRESULT,
    // ITaskbarList3 (9..20)
    pub set_progress_value: unsafe extern "system" fn(*mut c_void, HWND, u64, u64) -> HRESULT,
    pub set_progress_state: unsafe extern "system" fn(*mut c_void, HWND, u32) -> HRESULT,
    pub register_tab: unsafe extern "system" fn(*mut c_void, HWND, HWND) -> HRESULT,
    pub unregister_tab: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    pub set_tab_order: unsafe extern "system" fn(*mut c_void, HWND, HWND) -> HRESULT,
    pub set_tab_active: unsafe extern "system" fn(*mut c_void, HWND, HWND, u32) -> HRESULT,
    pub thumb_bar_add_buttons:
        unsafe extern "system" fn(*mut c_void, HWND, u32, *const THUMBBUTTON) -> HRESULT,
    pub thumb_bar_update_buttons:
        unsafe extern "system" fn(*mut c_void, HWND, u32, *const THUMBBUTTON) -> HRESULT,
    pub thumb_bar_set_image_list:
        unsafe extern "system" fn(*mut c_void, HWND, *mut c_void) -> HRESULT,
    pub set_overlay_icon:
        unsafe extern "system" fn(*mut c_void, HWND, HICON, *const u16) -> HRESULT,
    pub set_thumbnail_tooltip: unsafe extern "system" fn(*mut c_void, HWND, *const u16) -> HRESULT,
    pub set_thumbnail_clip: unsafe extern "system" fn(*mut c_void, HWND, *const c_void) -> HRESULT,
}

/// Exact 12-slot COM vtable for `ICustomDestinationList` (`IUnknown` + 9 methods).
#[repr(C)]
#[allow(dead_code)]
pub(crate) struct ICustomDestinationListVtbl {
    // IUnknown (0..2)
    pub query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    // ICustomDestinationList (3..11)
    pub set_app_id: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    pub begin_list:
        unsafe extern "system" fn(*mut c_void, *mut u32, *const Guid, *mut *mut c_void) -> HRESULT,
    pub append_category: unsafe extern "system" fn(*mut c_void, *const u16, *mut c_void) -> HRESULT,
    pub append_known_category: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    pub add_user_tasks: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    pub commit_list: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    pub get_removed_destinations:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HRESULT,
    pub delete_list: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    pub abort_list: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

/// Exact 9-slot COM vtable for `IObjectCollection` (`IUnknown` + `IObjectArray` + 4 methods).
#[repr(C)]
#[allow(dead_code)]
pub(crate) struct IObjectCollectionVtbl {
    pub query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    pub get_at:
        unsafe extern "system" fn(*mut c_void, u32, *const Guid, *mut *mut c_void) -> HRESULT,
    pub add_object: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    pub add_from_array: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    pub remove_object_at: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    pub clear: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

/// Exact 21-slot COM vtable for `IShellLinkW` (`IUnknown` + 18 methods).
#[repr(C)]
#[allow(dead_code)]
pub(crate) struct IShellLinkWVtbl {
    pub query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub get_path:
        unsafe extern "system" fn(*mut c_void, *mut u16, i32, *mut c_void, u32) -> HRESULT,
    pub get_id_list: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    pub set_id_list: unsafe extern "system" fn(*mut c_void, *const c_void) -> HRESULT,
    pub get_description: unsafe extern "system" fn(*mut c_void, *mut u16, i32) -> HRESULT,
    pub set_description: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    pub get_working_directory: unsafe extern "system" fn(*mut c_void, *mut u16, i32) -> HRESULT,
    pub set_working_directory: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    pub get_arguments: unsafe extern "system" fn(*mut c_void, *mut u16, i32) -> HRESULT,
    pub set_arguments: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    pub get_hotkey: unsafe extern "system" fn(*mut c_void, *mut u16) -> HRESULT,
    pub set_hotkey: unsafe extern "system" fn(*mut c_void, u16) -> HRESULT,
    pub get_show_cmd: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
    pub set_show_cmd: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
    pub get_icon_location:
        unsafe extern "system" fn(*mut c_void, *mut u16, i32, *mut i32) -> HRESULT,
    pub set_icon_location: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> HRESULT,
    pub set_relative_path: unsafe extern "system" fn(*mut c_void, *const u16, u32) -> HRESULT,
    pub resolve: unsafe extern "system" fn(*mut c_void, HWND, u32) -> HRESULT,
    pub set_path: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
}

/// Exact 8-slot COM vtable for `IPropertyStore` (`IUnknown` + 5 methods).
#[repr(C)]
#[allow(dead_code)]
pub(crate) struct IPropertyStoreVtbl {
    pub query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
    pub get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    pub get_at: unsafe extern "system" fn(*mut c_void, u32, *mut PropertyKey) -> HRESULT,
    pub get_value:
        unsafe extern "system" fn(*mut c_void, *const PropertyKey, *mut PropVariant) -> HRESULT,
    pub set_value:
        unsafe extern "system" fn(*mut c_void, *const PropertyKey, *const PropVariant) -> HRESULT,
    pub commit: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

// ---------------------------------------------------------------------------
// Win32 FFI imports
// ---------------------------------------------------------------------------

#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
mod ffi {
    use super::*;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct Point {
        pub x: i32,
        pub y: i32,
    }

    #[repr(C)]
    pub struct IconInfo {
        pub f_icon: i32,
        pub x_hotspot: u32,
        pub y_hotspot: u32,
        pub hbm_mask: HBITMAP,
        pub hbm_color: HBITMAP,
    }

    pub type SubclassProc = unsafe extern "system" fn(
        hwnd: HWND,
        u_msg: u32,
        w_param: usize,
        l_param: isize,
        u_id_subclass: usize,
        dw_ref_data: usize,
    ) -> isize;

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        pub fn DwmSetWindowAttribute(
            hwnd: HWND,
            dw_attribute: u32,
            pv_attribute: *const c_void,
            cb_attribute: u32,
        ) -> HRESULT;
    }

    #[link(name = "comctl32")]
    unsafe extern "system" {
        pub fn SetWindowSubclass(
            hwnd: HWND,
            pfn_subclass: SubclassProc,
            u_id_subclass: usize,
            dw_ref_data: usize,
        ) -> i32;
        pub fn RemoveWindowSubclass(
            hwnd: HWND,
            pfn_subclass: SubclassProc,
            u_id_subclass: usize,
        ) -> i32;
        pub fn DefSubclassProc(hwnd: HWND, u_msg: u32, w_param: usize, l_param: isize) -> isize;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn ScreenToClient(hwnd: HWND, lp_point: *mut Point) -> i32;
        pub fn IsZoomed(hwnd: HWND) -> i32;
        pub fn ShowWindow(hwnd: HWND, n_cmd_show: i32) -> i32;
        pub fn CreateIconIndirect(piconinfo: *const IconInfo) -> HICON;
        pub fn DestroyIcon(hicon: HICON) -> i32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        pub fn CreateBitmap(
            n_width: i32,
            n_height: i32,
            n_planes: u32,
            n_bit_count: u32,
            lp_bits: *const c_void,
        ) -> HBITMAP;
        pub fn DeleteObject(ho: *mut c_void) -> i32;
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        pub fn RegGetValueW(
            hkey: HKEY,
            lp_sub_key: *const u16,
            lp_value: *const u16,
            dw_flags: u32,
            pdw_type: *mut u32,
            pv_data: *mut c_void,
            pcb_data: *mut u32,
        ) -> u32;
        pub fn RegOpenKeyExW(
            hkey: HKEY,
            lp_sub_key: *const u16,
            ul_options: u32,
            sam_desired: u32,
            phk_result: *mut HKEY,
        ) -> u32;
        pub fn RegCreateKeyExW(
            hkey: HKEY,
            lp_sub_key: *const u16,
            reserved: u32,
            lp_class: *const u16,
            dw_options: u32,
            sam_desired: u32,
            lp_security_attributes: *const c_void,
            phk_result: *mut HKEY,
            lpdw_disposition: *mut u32,
        ) -> u32;
        pub fn RegSetValueExW(
            hkey: HKEY,
            lp_value_name: *const u16,
            reserved: u32,
            dw_type: u32,
            lp_data: *const u8,
            cb_data: u32,
        ) -> u32;
        pub fn RegDeleteTreeW(hkey: HKEY, lp_sub_key: *const u16) -> u32;
        pub fn RegNotifyChangeKeyValue(
            hkey: HKEY,
            b_watch_subtree: i32,
            dw_notify_filter: u32,
            h_event: HANDLE,
            f_asynchronous: i32,
        ) -> u32;
        pub fn RegCloseKey(hkey: HKEY) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn CreateEventW(
            lp_event_attributes: *const c_void,
            b_manual_reset: i32,
            b_initial_state: i32,
            lp_name: *const u16,
        ) -> HANDLE;
        pub fn SetEvent(h_event: HANDLE) -> i32;
        pub fn WaitForMultipleObjects(
            n_count: u32,
            lp_handles: *const HANDLE,
            b_wait_all: i32,
            dw_milliseconds: u32,
        ) -> u32;
        pub fn CloseHandle(h_object: HANDLE) -> i32;
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        pub fn CoInitializeEx(pv_reserved: *const c_void, dw_co_init: u32) -> HRESULT;
        pub fn CoUninitialize();
        pub fn CoCreateInstance(
            rclsid: *const Guid,
            p_unk_outer: *mut c_void,
            dw_cls_context: u32,
            riid: *const Guid,
            ppv: *mut *mut c_void,
        ) -> HRESULT;
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        pub fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> HRESULT;
    }
}

#[cfg(windows)]
fn to_wide_null(input: &str) -> Vec<u16> {
    input.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
struct ComInitGuard {
    should_uninit: bool,
}

#[cfg(windows)]
impl ComInitGuard {
    fn init_sta() -> Result<Self, HRESULT> {
        let hr = unsafe {
            ffi::CoInitializeEx(
                ptr::null_mut(),
                COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE,
            )
        };
        if hr == S_OK || hr == S_FALSE {
            Ok(Self {
                should_uninit: true,
            })
        } else if hr == RPC_E_CHANGED_MODE {
            // Thread already initialized COM under another apartment model.
            Ok(Self {
                should_uninit: false,
            })
        } else {
            Err(hr)
        }
    }
}

#[cfg(windows)]
impl Drop for ComInitGuard {
    fn drop(&mut self) {
        if self.should_uninit {
            unsafe {
                ffi::CoUninitialize();
            }
        }
    }
}

#[cfg(windows)]
struct ComPtr {
    raw: *mut c_void,
}

#[cfg(windows)]
impl ComPtr {
    fn new(raw: *mut c_void) -> Option<Self> {
        (!raw.is_null()).then_some(Self { raw })
    }

    fn as_ptr(&self) -> *mut c_void {
        self.raw
    }

    unsafe fn vtbl<T>(&self) -> &T {
        unsafe { &*(*self.raw.cast::<*const T>()) }
    }
}

#[cfg(windows)]
impl Drop for ComPtr {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                let vtbl = self.vtbl::<IUnknownVtbl>();
                (vtbl.release)(self.raw);
            }
            self.raw = ptr::null_mut();
        }
    }
}

#[cfg(windows)]
struct RegKeyGuard(HKEY);

#[cfg(windows)]
impl Drop for RegKeyGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                ffi::RegCloseKey(self.0);
            }
            self.0 = ptr::null_mut();
        }
    }
}

// ---------------------------------------------------------------------------
// 1. Windows 11 DWM Attributes & AppUserModelID
// ---------------------------------------------------------------------------

/// Registers the process AppUserModelID so Taskbar grouping, Jump Lists, and
/// Toast notifications resolve to the same application identity.
pub fn set_process_app_user_model_id(app_id: &str) -> bool {
    #[cfg(windows)]
    {
        if cfg!(test) || app_id.trim().is_empty() {
            return false;
        }
        let wide = to_wide_null(app_id.trim());
        let hr = unsafe { ffi::SetCurrentProcessExplicitAppUserModelID(wide.as_ptr()) };
        hr >= 0
    }
    #[cfg(not(windows))]
    {
        let _ = app_id;
        false
    }
}

/// Applies Windows 11 DWM attributes to `hwnd`:
/// - `DWMWA_USE_IMMERSIVE_DARK_MODE` (`20`, fallback `19`)
/// - `DWMWA_WINDOW_CORNER_PREFERENCE` (`33`) = `DWMWCP_ROUND` (`2`)
/// - `DWMWA_SYSTEMBACKDROP_TYPE` (`38`) = `DWMSBT_MAINWINDOW` (`2`) or `DWMSBT_NONE` (`1`)
pub fn apply_dwm_window_attributes(hwnd: isize, dark_mode: bool, mica_backdrop: bool) {
    #[cfg(windows)]
    {
        if cfg!(test) || hwnd == 0 {
            return;
        }
        let hwnd_ptr = hwnd as HWND;

        let dark_val: i32 = i32::from(dark_mode);
        let hr = unsafe {
            ffi::DwmSetWindowAttribute(
                hwnd_ptr,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&raw const dark_val).cast::<c_void>(),
                std::mem::size_of::<i32>() as u32,
            )
        };
        if hr < 0 {
            let _ = unsafe {
                ffi::DwmSetWindowAttribute(
                    hwnd_ptr,
                    DWMWA_USE_IMMERSIVE_DARK_MODE_BEFORE_20H1,
                    (&raw const dark_val).cast::<c_void>(),
                    std::mem::size_of::<i32>() as u32,
                )
            };
        }

        let corner_pref: u32 = DWMWCP_ROUND;
        let _ = unsafe {
            ffi::DwmSetWindowAttribute(
                hwnd_ptr,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const corner_pref).cast::<c_void>(),
                std::mem::size_of::<u32>() as u32,
            )
        };

        let backdrop: u32 = if mica_backdrop {
            DWMSBT_MAINWINDOW
        } else {
            DWMSBT_NONE
        };
        let _ = unsafe {
            ffi::DwmSetWindowAttribute(
                hwnd_ptr,
                DWMWA_SYSTEMBACKDROP_TYPE,
                (&raw const backdrop).cast::<c_void>(),
                std::mem::size_of::<u32>() as u32,
            )
        };
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, dark_mode, mica_backdrop);
    }
}

// ---------------------------------------------------------------------------
// 2. Snap Layouts (`WM_NCHITTEST` `HTMAXBUTTON`) & Taskbar `THBN_CLICKED` Subclass
// ---------------------------------------------------------------------------

/// Rectangle in client physical pixels (`egui::Rect` scaled by `pixels_per_point`)
/// identifying the custom titlebar Maximize button for Windows 11 Snap Layouts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SnapLayoutRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl SnapLayoutRect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn is_valid(self) -> bool {
        self.right > self.left && self.bottom > self.top
    }

    pub const fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// Commands emitted when the user clicks a Windows Taskbar Thumbnail Toolbar button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarThumbCommand {
    Previous,
    PlayPause,
    Next,
}

#[cfg(windows)]
struct WindowSubclassState {
    maximize_rect: Option<SnapLayoutRect>,
    thumb_tx: mpsc::Sender<TaskbarThumbCommand>,
}

#[cfg(windows)]
fn subclass_registry() -> &'static Mutex<HashMap<isize, WindowSubclassState>> {
    static REGISTRY: OnceLock<Mutex<HashMap<isize, WindowSubclassState>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(windows)]
fn thumb_command_from_button_id(button_id: u16) -> Option<TaskbarThumbCommand> {
    match u32::from(button_id) {
        THUMB_BTN_PREV_ID => Some(TaskbarThumbCommand::Previous),
        THUMB_BTN_PLAY_PAUSE_ID => Some(TaskbarThumbCommand::PlayPause),
        THUMB_BTN_NEXT_ID => Some(TaskbarThumbCommand::Next),
        _ => None,
    }
}

#[cfg(windows)]
unsafe extern "system" fn fasttube_subclass_proc(
    hwnd: HWND,
    u_msg: u32,
    w_param: usize,
    l_param: isize,
    u_id_subclass: usize,
    _dw_ref_data: usize,
) -> isize {
    let hwnd_key = hwnd as isize;
    match u_msg {
        WM_NCHITTEST => {
            let max_rect = subclass_registry()
                .try_lock()
                .ok()
                .and_then(|map| map.get(&hwnd_key).and_then(|s| s.maximize_rect));
            if let Some(rect) = max_rect
                && rect.is_valid()
            {
                let mut pt = ffi::Point {
                    x: i32::from((l_param & 0xFFFF) as i16),
                    y: i32::from(((l_param >> 16) & 0xFFFF) as i16),
                };
                if unsafe { ffi::ScreenToClient(hwnd, &raw mut pt) } != 0
                    && rect.contains(pt.x, pt.y)
                {
                    return HTMAXBUTTON;
                }
            }
        }
        WM_NCLBUTTONDOWN if w_param == HTMAXBUTTON as usize => {
            // Suppress default non-client chrome painting on frameless windows while
            // keeping Windows 11 Snap Layouts hover flyout active.
            return 0;
        }
        WM_NCLBUTTONUP if w_param == HTMAXBUTTON as usize => {
            let show_cmd = if unsafe { ffi::IsZoomed(hwnd) } != 0 {
                SW_RESTORE
            } else {
                SW_MAXIMIZE
            };
            unsafe {
                ffi::ShowWindow(hwnd, show_cmd);
            }
            return 0;
        }
        WM_COMMAND => {
            let hiword = ((w_param >> 16) & 0xFFFF) as u16;
            let loword = (w_param & 0xFFFF) as u16;
            if hiword == THBN_CLICKED
                && let Some(cmd) = thumb_command_from_button_id(loword)
            {
                let tx = subclass_registry()
                    .try_lock()
                    .ok()
                    .and_then(|map| map.get(&hwnd_key).map(|s| s.thumb_tx.clone()));
                if let Some(tx) = tx {
                    let _ = tx.send(cmd);
                }
                return 0;
            }
        }
        WM_NCDESTROY => {
            if let Ok(mut map) = subclass_registry().lock() {
                map.remove(&hwnd_key);
            }
            unsafe {
                ffi::RemoveWindowSubclass(hwnd, fasttube_subclass_proc, u_id_subclass);
            }
        }
        _ => {}
    }

    unsafe { ffi::DefSubclassProc(hwnd, u_msg, w_param, l_param) }
}

/// Installs the Win32 window subclass on `hwnd` for Windows 11 Snap Layouts
/// (`HTMAXBUTTON`) and Taskbar Thumbnail Toolbar (`THBN_CLICKED`) handling.
#[cfg_attr(test, allow(dead_code))]
pub fn install_window_subclass(hwnd: isize, thumb_tx: mpsc::Sender<TaskbarThumbCommand>) -> bool {
    #[cfg(windows)]
    {
        if cfg!(test) || hwnd == 0 {
            return false;
        }
        let already_installed = if let Ok(mut map) = subclass_registry().lock() {
            if let Some(existing) = map.get_mut(&hwnd) {
                existing.thumb_tx = thumb_tx;
                true
            } else {
                map.insert(
                    hwnd,
                    WindowSubclassState {
                        maximize_rect: None,
                        thumb_tx,
                    },
                );
                false
            }
        } else {
            return false;
        };
        if already_installed {
            return true;
        }
        let ok = unsafe {
            ffi::SetWindowSubclass(
                hwnd as HWND,
                fasttube_subclass_proc,
                FASTTUBE_SUBCLASS_ID,
                0,
            ) != 0
        };
        if !ok && let Ok(mut map) = subclass_registry().lock() {
            map.remove(&hwnd);
        }
        ok
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, thumb_tx);
        false
    }
}

/// Updates the active maximize button hit-test rectangle (client physical pixels)
/// for `hwnd`. Pass `None` when the button is hidden (e.g. fullscreen/native titlebar).
pub fn update_maximize_button_rect(hwnd: isize, rect: Option<SnapLayoutRect>) {
    #[cfg(windows)]
    {
        if cfg!(test) || hwnd == 0 {
            return;
        }
        if let Ok(mut map) = subclass_registry().lock()
            && let Some(state) = map.get_mut(&hwnd)
        {
            state.maximize_rect = rect.filter(|r| r.is_valid());
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, rect);
    }
}

/// Removes the Win32 window subclass installed on `hwnd`.
pub fn remove_window_subclass(hwnd: isize) {
    #[cfg(windows)]
    {
        if cfg!(test) || hwnd == 0 {
            return;
        }
        let existed = subclass_registry()
            .lock()
            .ok()
            .and_then(|mut map| map.remove(&hwnd))
            .is_some();
        if existed {
            unsafe {
                ffi::RemoveWindowSubclass(
                    hwnd as HWND,
                    fasttube_subclass_proc,
                    FASTTUBE_SUBCLASS_ID,
                );
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
    }
}

// ---------------------------------------------------------------------------
// 3. Live Windows System Theme Reader & Zero-Polling Registry Observer
// ---------------------------------------------------------------------------

/// Reads `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme`.
/// Returns `true` when Windows apps are configured to use dark theme (`0`), `false`
/// when light theme (`1`), and defaults to `true` on error or non-Windows platforms.
pub fn read_windows_apps_use_dark_theme() -> bool {
    #[cfg(windows)]
    {
        if cfg!(test) {
            return true;
        }
        let subkey = to_wide_null(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        let value_name = to_wide_null("AppsUseLightTheme");
        let mut data: u32 = 0;
        let mut data_size: u32 = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            ffi::RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                value_name.as_ptr(),
                RRF_RT_REG_DWORD,
                ptr::null_mut(),
                (&raw mut data).cast::<c_void>(),
                &raw mut data_size,
            )
        };
        if status == ERROR_SUCCESS {
            data == 0
        } else {
            true
        }
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Zero-polling background observer for Windows Dark/Light mode changes via
/// `RegNotifyChangeKeyValue` on `HKCU\...\Themes\Personalize`.
pub struct SystemThemeObserver {
    is_dark: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    #[cfg(windows)]
    stop_event: isize,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl SystemThemeObserver {
    #[cfg_attr(test, allow(dead_code))]
    pub fn start(ctx: &egui::Context) -> Self {
        let initial_dark = read_windows_apps_use_dark_theme();
        let is_dark = Arc::new(AtomicBool::new(initial_dark));
        let stop_flag = Arc::new(AtomicBool::new(false));

        #[cfg(windows)]
        {
            if cfg!(test) {
                return Self {
                    is_dark,
                    stop_flag,
                    stop_event: 0,
                    worker: None,
                };
            }

            let stop_event_handle = unsafe { ffi::CreateEventW(ptr::null(), 1, 0, ptr::null()) };
            if stop_event_handle.is_null() {
                return Self {
                    is_dark,
                    stop_flag,
                    stop_event: 0,
                    worker: None,
                };
            }
            let stop_event_raw = stop_event_handle as isize;
            let is_dark_bg = Arc::clone(&is_dark);
            let stop_flag_bg = Arc::clone(&stop_flag);
            let ctx_bg = ctx.clone();

            let worker = std::thread::Builder::new()
                .name("fasttube-theme-observer".to_owned())
                .spawn(move || {
                    let subkey = to_wide_null(
                        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
                    );
                    let mut hkey: HKEY = ptr::null_mut();
                    let open_status = unsafe {
                        ffi::RegOpenKeyExW(
                            HKEY_CURRENT_USER,
                            subkey.as_ptr(),
                            0,
                            KEY_NOTIFY | KEY_QUERY_VALUE,
                            &raw mut hkey,
                        )
                    };
                    if open_status != ERROR_SUCCESS || hkey.is_null() {
                        return;
                    }
                    let _key_guard = RegKeyGuard(hkey);

                    let notify_event = unsafe { ffi::CreateEventW(ptr::null(), 0, 0, ptr::null()) };
                    if notify_event.is_null() {
                        return;
                    }

                    let handles = [stop_event_raw as HANDLE, notify_event];
                    while !stop_flag_bg.load(Ordering::Acquire) {
                        let reg_status = unsafe {
                            ffi::RegNotifyChangeKeyValue(
                                hkey,
                                0,
                                REG_NOTIFY_CHANGE_LAST_SET,
                                notify_event,
                                1,
                            )
                        };
                        if reg_status != ERROR_SUCCESS {
                            break;
                        }
                        let wait = unsafe {
                            ffi::WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE)
                        };
                        if wait == WAIT_OBJECT_0 || stop_flag_bg.load(Ordering::Acquire) {
                            break;
                        }
                        if wait == WAIT_OBJECT_0 + 1 {
                            let latest = read_windows_apps_use_dark_theme();
                            let prev = is_dark_bg.swap(latest, Ordering::AcqRel);
                            if prev != latest {
                                ctx_bg.request_repaint();
                            }
                        } else {
                            break;
                        }
                    }

                    unsafe {
                        ffi::CloseHandle(notify_event);
                    }
                })
                .ok();

            if worker.is_none() {
                unsafe {
                    ffi::CloseHandle(stop_event_handle);
                }
                return Self {
                    is_dark,
                    stop_flag,
                    stop_event: 0,
                    worker: None,
                };
            }

            Self {
                is_dark,
                stop_flag,
                stop_event: stop_event_raw,
                worker,
            }
        }

        #[cfg(not(windows))]
        {
            let _ = ctx;
            Self {
                is_dark,
                stop_flag,
                worker: None,
            }
        }
    }

    pub fn is_dark(&self) -> bool {
        self.is_dark.load(Ordering::Acquire)
    }

    #[allow(dead_code)]
    pub fn is_dark_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.is_dark)
    }
}

impl Drop for SystemThemeObserver {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        #[cfg(windows)]
        if self.stop_event != 0 {
            unsafe {
                ffi::SetEvent(self.stop_event as HANDLE);
            }
        }
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        #[cfg(windows)]
        if self.stop_event != 0 {
            unsafe {
                ffi::CloseHandle(self.stop_event as HANDLE);
            }
            self.stop_event = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// 4. Taskbar Progress & Thumbnail Toolbar (`ITaskbarList3`)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TaskbarProgressState {
    NoProgress,
    Indeterminate,
    Normal,
    Error,
    Paused,
}

#[cfg(windows)]
impl TaskbarProgressState {
    fn to_tbp_flag(self) -> u32 {
        match self {
            Self::NoProgress => TBPF_NOPROGRESS,
            Self::Indeterminate => TBPF_INDETERMINATE,
            Self::Normal => TBPF_NORMAL,
            Self::Error => TBPF_ERROR,
            Self::Paused => TBPF_PAUSED,
        }
    }
}

#[cfg(windows)]
struct ThumbIcons {
    prev: HICON,
    play: HICON,
    pause: HICON,
    next: HICON,
}

#[cfg(windows)]
impl ThumbIcons {
    fn create() -> Self {
        Self {
            prev: create_glyph_icon(GlyphKind::Previous),
            play: create_glyph_icon(GlyphKind::Play),
            pause: create_glyph_icon(GlyphKind::Pause),
            next: create_glyph_icon(GlyphKind::Next),
        }
    }
}

#[cfg(windows)]
impl Drop for ThumbIcons {
    fn drop(&mut self) {
        for icon in [self.prev, self.play, self.pause, self.next] {
            if !icon.is_null() {
                unsafe {
                    ffi::DestroyIcon(icon);
                }
            }
        }
    }
}

#[cfg(windows)]
#[derive(Clone, Copy)]
enum GlyphKind {
    Previous,
    Play,
    Pause,
    Next,
}

#[cfg(windows)]
fn create_glyph_icon(kind: GlyphKind) -> HICON {
    const SIZE: usize = 16;
    let mut bgra = vec![0u8; SIZE * SIZE * 4];
    let mut set_px = |x: usize, y: usize| {
        if x < SIZE && y < SIZE {
            let idx = (y * SIZE + x) * 4;
            bgra[idx] = 255; // B
            bgra[idx + 1] = 255; // G
            bgra[idx + 2] = 255; // R
            bgra[idx + 3] = 255; // A
        }
    };

    match kind {
        GlyphKind::Play => {
            for y in 3..=12 {
                let half = if y <= 7 { y - 3 } else { 12 - y };
                let max_x = 4 + (half * 2).min(8);
                for x in 4..=max_x {
                    set_px(x, y);
                }
            }
        }
        GlyphKind::Pause => {
            for y in 3..=12 {
                for x in 4..=6 {
                    set_px(x, y);
                }
                for x in 9..=11 {
                    set_px(x, y);
                }
            }
        }
        GlyphKind::Previous => {
            for y in 3..=12 {
                for x in 3..=4 {
                    set_px(x, y);
                }
                let half = if y <= 7 { y - 3 } else { 12 - y };
                let min_x = 12usize.saturating_sub((half * 2).min(6));
                for x in min_x..=12 {
                    set_px(x, y);
                }
            }
        }
        GlyphKind::Next => {
            for y in 3..=12 {
                let half = if y <= 7 { y - 3 } else { 12 - y };
                let max_x = 3 + (half * 2).min(6);
                for x in 3..=max_x {
                    set_px(x, y);
                }
                for x in 11..=12 {
                    set_px(x, y);
                }
            }
        }
    }

    let mask = [0u8; SIZE * 2];
    let hbm_color =
        unsafe { ffi::CreateBitmap(SIZE as i32, SIZE as i32, 1, 32, bgra.as_ptr().cast()) };
    if hbm_color.is_null() {
        return ptr::null_mut();
    }
    let hbm_mask =
        unsafe { ffi::CreateBitmap(SIZE as i32, SIZE as i32, 1, 1, mask.as_ptr().cast()) };
    if hbm_mask.is_null() {
        unsafe {
            ffi::DeleteObject(hbm_color);
        }
        return ptr::null_mut();
    }
    let info = ffi::IconInfo {
        f_icon: 1,
        x_hotspot: 0,
        y_hotspot: 0,
        hbm_mask,
        hbm_color,
    };
    let icon = unsafe { ffi::CreateIconIndirect(&raw const info) };
    unsafe {
        ffi::DeleteObject(hbm_color);
        ffi::DeleteObject(hbm_mask);
    }
    icon
}

#[cfg(windows)]
fn write_tooltip_utf16(dest: &mut [u16; 260], text: &str) {
    dest.fill(0);
    for (slot, unit) in dest.iter_mut().take(259).zip(text.encode_utf16()) {
        *slot = unit;
    }
}

/// Manages `ITaskbarList3` progress reporting and thumbnail toolbar buttons
/// with state diff-deduplication to avoid redundant COM calls.
pub struct TaskbarController {
    // Field drop order matters: release COM interface before `CoUninitialize`.
    #[cfg(windows)]
    taskbar: Option<ComPtr>,
    #[cfg(windows)]
    _com_guard: Option<ComInitGuard>,
    #[cfg(windows)]
    init_attempted: bool,
    #[cfg(windows)]
    icons: Option<ThumbIcons>,
    #[cfg(windows)]
    thumb_added_hwnd: Option<isize>,
    last_progress: Option<(isize, TaskbarProgressState, u64, u64)>,
    last_thumb_state: Option<(isize, bool, bool, bool, bool, bool)>,
}

impl Default for TaskbarController {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskbarController {
    pub fn new() -> Self {
        Self {
            #[cfg(windows)]
            taskbar: None,
            #[cfg(windows)]
            _com_guard: None,
            #[cfg(windows)]
            init_attempted: false,
            #[cfg(windows)]
            icons: None,
            #[cfg(windows)]
            thumb_added_hwnd: None,
            last_progress: None,
            last_thumb_state: None,
        }
    }

    #[cfg(windows)]
    fn ensure_initialized(&mut self) -> Option<&ComPtr> {
        if self.init_attempted {
            return self.taskbar.as_ref();
        }
        self.init_attempted = true;
        if cfg!(test) {
            return None;
        }
        let Ok(guard) = ComInitGuard::init_sta() else {
            return None;
        };
        let mut raw: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            ffi::CoCreateInstance(
                &raw const CLSID_TASKBAR_LIST,
                ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &raw const IID_ITASKBAR_LIST3,
                &raw mut raw,
            )
        };
        if hr < 0 {
            return None;
        }
        let ptr = ComPtr::new(raw)?;
        let init_hr = unsafe {
            let vtbl = ptr.vtbl::<ITaskbarList3Vtbl>();
            (vtbl.hr_init)(ptr.as_ptr())
        };
        if init_hr < 0 {
            return None;
        }
        self._com_guard = Some(guard);
        self.taskbar = Some(ptr);
        self.taskbar.as_ref()
    }

    /// Updates the Windows taskbar progress indicator on `hwnd`, skipping
    /// redundant COM calls when `(hwnd, state, completed, total)` is unchanged.
    pub fn set_progress(
        &mut self,
        hwnd: isize,
        state: TaskbarProgressState,
        completed: u64,
        total: u64,
    ) {
        let normalized = match state {
            TaskbarProgressState::NoProgress | TaskbarProgressState::Indeterminate => {
                (hwnd, state, 0, 0)
            }
            TaskbarProgressState::Normal
            | TaskbarProgressState::Error
            | TaskbarProgressState::Paused => {
                let safe_total = total.max(1);
                (hwnd, state, completed.min(safe_total), safe_total)
            }
        };
        if self.last_progress == Some(normalized) {
            return;
        }
        self.last_progress = Some(normalized);

        #[cfg(windows)]
        {
            if cfg!(test) || hwnd == 0 {
                return;
            }
            let Some(taskbar) = self.ensure_initialized() else {
                return;
            };
            let hwnd_ptr = hwnd as HWND;
            unsafe {
                let vtbl = taskbar.vtbl::<ITaskbarList3Vtbl>();
                let _ = (vtbl.set_progress_state)(taskbar.as_ptr(), hwnd_ptr, state.to_tbp_flag());
                if matches!(
                    state,
                    TaskbarProgressState::Normal
                        | TaskbarProgressState::Error
                        | TaskbarProgressState::Paused
                ) {
                    let _ = (vtbl.set_progress_value)(
                        taskbar.as_ptr(),
                        hwnd_ptr,
                        normalized.2,
                        normalized.3,
                    );
                }
            }
        }
    }

    /// Adds or updates the 3 taskbar thumbnail toolbar buttons (`Previous`,
    /// `Play/Pause`, `Next`) with bilingual tooltips and state diff-deduplication.
    pub fn sync_thumb_toolbar(
        &mut self,
        hwnd: isize,
        has_video: bool,
        is_playing: bool,
        can_prev: bool,
        can_next: bool,
        pt: bool,
    ) {
        let state_key = (hwnd, has_video, is_playing, can_prev, can_next, pt);
        if self.last_thumb_state == Some(state_key) {
            return;
        }
        self.last_thumb_state = Some(state_key);

        #[cfg(windows)]
        {
            if cfg!(test) || hwnd == 0 {
                return;
            }
            let (icon_prev, icon_play, icon_pause, icon_next) = {
                let icons = self.icons.get_or_insert_with(ThumbIcons::create);
                (icons.prev, icons.play, icons.pause, icons.next)
            };
            let already_added = self.thumb_added_hwnd == Some(hwnd);
            let Some(taskbar) = self.ensure_initialized() else {
                return;
            };

            let prev_tip = if pt {
                "Vídeo anterior"
            } else {
                "Previous video"
            };
            let play_tip = match (is_playing, pt) {
                (true, true) => "Pausar",
                (true, false) => "Pause",
                (false, true) => "Reproduzir",
                (false, false) => "Play",
            };
            let next_tip = if pt { "Próximo vídeo" } else { "Next video" };

            let make_button = |id: u32, icon: HICON, tip: &str, flags: u32| -> THUMBBUTTON {
                let mut sz_tip = [0u16; 260];
                write_tooltip_utf16(&mut sz_tip, tip);
                let mut mask = THB_TOOLTIP | THB_FLAGS;
                if !icon.is_null() {
                    mask |= THB_ICON;
                }
                THUMBBUTTON {
                    dw_mask: mask,
                    i_id: id,
                    i_bitmap: 0,
                    h_icon: icon,
                    sz_tip,
                    dw_flags: flags,
                }
            };

            let prev_flags = if !has_video {
                THBF_HIDDEN
            } else if can_prev {
                THBF_ENABLED
            } else {
                THBF_DISABLED
            };
            let play_flags = if has_video { THBF_ENABLED } else { THBF_HIDDEN };
            let next_flags = if !has_video {
                THBF_HIDDEN
            } else if can_next {
                THBF_ENABLED
            } else {
                THBF_DISABLED
            };

            let buttons = [
                make_button(THUMB_BTN_PREV_ID, icon_prev, prev_tip, prev_flags),
                make_button(
                    THUMB_BTN_PLAY_PAUSE_ID,
                    if is_playing { icon_pause } else { icon_play },
                    play_tip,
                    play_flags,
                ),
                make_button(THUMB_BTN_NEXT_ID, icon_next, next_tip, next_flags),
            ];

            let hwnd_ptr = hwnd as HWND;
            let hr = unsafe {
                let vtbl = taskbar.vtbl::<ITaskbarList3Vtbl>();
                if already_added {
                    (vtbl.thumb_bar_update_buttons)(
                        taskbar.as_ptr(),
                        hwnd_ptr,
                        buttons.len() as u32,
                        buttons.as_ptr(),
                    )
                } else {
                    (vtbl.thumb_bar_add_buttons)(
                        taskbar.as_ptr(),
                        hwnd_ptr,
                        buttons.len() as u32,
                        buttons.as_ptr(),
                    )
                }
            };
            if hr >= 0 && !already_added {
                self.thumb_added_hwnd = Some(hwnd);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 5. Windows Jump Lists (`ICustomDestinationList`)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JumpListTask {
    pub title: String,
    pub args: String,
    pub description: String,
}

/// Returns the default bilingual FastTube taskbar Jump List tasks using
/// `fasttube://section/...` URIs.
pub fn default_jump_list_tasks(pt: bool) -> Vec<JumpListTask> {
    if pt {
        vec![
            JumpListTask {
                title: "Início".to_owned(),
                args: "fasttube://section/home".to_owned(),
                description: "Abrir a página inicial do FastTube".to_owned(),
            },
            JumpListTask {
                title: "Inscrições".to_owned(),
                args: "fasttube://section/subscriptions".to_owned(),
                description: "Abrir o feed de inscrições".to_owned(),
            },
            JumpListTask {
                title: "Histórico".to_owned(),
                args: "fasttube://section/history".to_owned(),
                description: "Abrir o histórico de reprodução".to_owned(),
            },
            JumpListTask {
                title: "Downloads".to_owned(),
                args: "fasttube://section/downloads".to_owned(),
                description: "Abrir downloads offline".to_owned(),
            },
            JumpListTask {
                title: "Assistir mais tarde".to_owned(),
                args: "fasttube://section/watch_later".to_owned(),
                description: "Abrir a lista Assistir mais tarde".to_owned(),
            },
        ]
    } else {
        vec![
            JumpListTask {
                title: "Home".to_owned(),
                args: "fasttube://section/home".to_owned(),
                description: "Open FastTube Home feed".to_owned(),
            },
            JumpListTask {
                title: "Subscriptions".to_owned(),
                args: "fasttube://section/subscriptions".to_owned(),
                description: "Open Subscriptions feed".to_owned(),
            },
            JumpListTask {
                title: "History".to_owned(),
                args: "fasttube://section/history".to_owned(),
                description: "Open watch history".to_owned(),
            },
            JumpListTask {
                title: "Downloads".to_owned(),
                args: "fasttube://section/downloads".to_owned(),
                description: "Open offline downloads".to_owned(),
            },
            JumpListTask {
                title: "Watch Later".to_owned(),
                args: "fasttube://section/watch_later".to_owned(),
                description: "Open Watch Later playlist".to_owned(),
            },
        ]
    }
}

/// Registers taskbar Jump List user tasks via `ICustomDestinationList`.
/// Best-effort; returns sanitized error messages without leaking filesystem paths.
#[cfg_attr(test, allow(dead_code))]
pub fn register_jump_list(exe_path: &Path, tasks: &[JumpListTask]) -> Result<(), String> {
    #[cfg(windows)]
    {
        if cfg!(test) || tasks.is_empty() {
            return Ok(());
        }
        let _com_guard = ComInitGuard::init_sta()
            .map_err(|hr| format!("COM initialization failed (0x{:08X})", hr as u32))?;

        let mut dest_raw: *mut c_void = ptr::null_mut();
        let hr = unsafe {
            ffi::CoCreateInstance(
                &raw const CLSID_DESTINATION_LIST,
                ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &raw const IID_ICUSTOM_DESTINATION_LIST,
                &raw mut dest_raw,
            )
        };
        if hr < 0 {
            return Err(format!(
                "ICustomDestinationList creation failed (0x{:08X})",
                hr as u32
            ));
        }
        let dest_list = ComPtr::new(dest_raw)
            .ok_or_else(|| "ICustomDestinationList returned null".to_owned())?;

        let app_id_wide = to_wide_null(FASTTUBE_APP_USER_MODEL_ID);
        let _ = unsafe {
            let vtbl = dest_list.vtbl::<ICustomDestinationListVtbl>();
            (vtbl.set_app_id)(dest_list.as_ptr(), app_id_wide.as_ptr())
        };

        let mut min_slots: u32 = 0;
        let mut removed_raw: *mut c_void = ptr::null_mut();
        let begin_hr = unsafe {
            let vtbl = dest_list.vtbl::<ICustomDestinationListVtbl>();
            (vtbl.begin_list)(
                dest_list.as_ptr(),
                &raw mut min_slots,
                &raw const IID_IOBJECT_ARRAY,
                &raw mut removed_raw,
            )
        };
        let _removed_guard = ComPtr::new(removed_raw);
        if begin_hr < 0 {
            return Err(format!(
                "Jump List BeginList failed (0x{:08X})",
                begin_hr as u32
            ));
        }

        let mut coll_raw: *mut c_void = ptr::null_mut();
        let coll_hr = unsafe {
            ffi::CoCreateInstance(
                &raw const CLSID_ENUMERABLE_OBJECT_COLLECTION,
                ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &raw const IID_IOBJECT_COLLECTION,
                &raw mut coll_raw,
            )
        };
        if coll_hr < 0 {
            unsafe {
                let vtbl = dest_list.vtbl::<ICustomDestinationListVtbl>();
                let _ = (vtbl.abort_list)(dest_list.as_ptr());
            }
            return Err(format!(
                "IObjectCollection creation failed (0x{:08X})",
                coll_hr as u32
            ));
        }
        let collection =
            ComPtr::new(coll_raw).ok_or_else(|| "IObjectCollection returned null".to_owned())?;

        let exe_wide = to_wide_null(&exe_path.to_string_lossy());
        for task in tasks {
            let mut link_raw: *mut c_void = ptr::null_mut();
            let link_hr = unsafe {
                ffi::CoCreateInstance(
                    &raw const CLSID_SHELL_LINK,
                    ptr::null_mut(),
                    CLSCTX_INPROC_SERVER,
                    &raw const IID_ISHELL_LINK_W,
                    &raw mut link_raw,
                )
            };
            if link_hr < 0 {
                continue;
            }
            let Some(link) = ComPtr::new(link_raw) else {
                continue;
            };
            let args_wide = to_wide_null(&task.args);
            let desc_wide = to_wide_null(&task.description);
            let title_wide = to_wide_null(&task.title);

            unsafe {
                let link_vtbl = link.vtbl::<IShellLinkWVtbl>();
                if (link_vtbl.set_path)(link.as_ptr(), exe_wide.as_ptr()) < 0 {
                    continue;
                }
                let _ = (link_vtbl.set_arguments)(link.as_ptr(), args_wide.as_ptr());
                let _ = (link_vtbl.set_description)(link.as_ptr(), desc_wide.as_ptr());
                let _ = (link_vtbl.set_icon_location)(link.as_ptr(), exe_wide.as_ptr(), 0);

                let mut prop_raw: *mut c_void = ptr::null_mut();
                if (link_vtbl.query_interface)(
                    link.as_ptr(),
                    &raw const IID_IPROPERTY_STORE,
                    &raw mut prop_raw,
                ) >= 0
                    && let Some(prop_store) = ComPtr::new(prop_raw)
                {
                    let prop_var = PropVariant {
                        vt: VT_LPWSTR,
                        w_reserved1: 0,
                        w_reserved2: 0,
                        w_reserved3: 0,
                        data: [
                            title_wide.as_ptr().cast_mut().cast::<c_void>(),
                            ptr::null_mut(),
                        ],
                    };
                    let prop_vtbl = prop_store.vtbl::<IPropertyStoreVtbl>();
                    let _ = (prop_vtbl.set_value)(
                        prop_store.as_ptr(),
                        &raw const PKEY_TITLE,
                        &raw const prop_var,
                    );
                    let _ = (prop_vtbl.commit)(prop_store.as_ptr());
                }

                let coll_vtbl = collection.vtbl::<IObjectCollectionVtbl>();
                let _ = (coll_vtbl.add_object)(collection.as_ptr(), link.as_ptr());
            }
        }

        let add_hr = unsafe {
            let vtbl = dest_list.vtbl::<ICustomDestinationListVtbl>();
            (vtbl.add_user_tasks)(dest_list.as_ptr(), collection.as_ptr())
        };
        if add_hr < 0 {
            unsafe {
                let vtbl = dest_list.vtbl::<ICustomDestinationListVtbl>();
                let _ = (vtbl.abort_list)(dest_list.as_ptr());
            }
            return Err(format!(
                "Jump List AddUserTasks failed (0x{:08X})",
                add_hr as u32
            ));
        }

        let commit_hr = unsafe {
            let vtbl = dest_list.vtbl::<ICustomDestinationListVtbl>();
            (vtbl.commit_list)(dest_list.as_ptr())
        };
        if commit_hr < 0 {
            return Err(format!(
                "Jump List CommitList failed (0x{:08X})",
                commit_hr as u32
            ));
        }

        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (exe_path, tasks);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 6. Native Windows Toast Notifications
// ---------------------------------------------------------------------------

fn escape_xml(input: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(input.len().min(max_chars));
    for ch in input.chars().take(max_chars) {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' | '\r' | '\t' => out.push(ch),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Builds a strictly XML-escaped Windows Toast payload.
pub fn build_toast_xml(title: &str, body: &str, launch_arg: Option<&str>) -> String {
    let escaped_title = escape_xml(title.trim(), MAX_TOAST_TITLE_CHARS);
    let escaped_body = escape_xml(body.trim(), MAX_TOAST_BODY_CHARS);
    let launch_attr = launch_arg
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|arg| format!(" launch=\"{}\"", escape_xml(arg, 512)))
        .unwrap_or_default();

    format!(
        "<toast{launch_attr}><visual><binding template=\"ToastGeneric\"><text>{escaped_title}</text><text>{escaped_body}</text></binding></visual></toast>"
    )
}

#[cfg(windows)]
fn toast_rate_limiter() -> &'static Mutex<Option<Instant>> {
    static LAST_SENT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    LAST_SENT.get_or_init(|| Mutex::new(None))
}

/// Dispatches a native Windows Toast notification on a background thread using
/// the WinRT `ToastNotificationManager`. Rate-limited and strictly disabled in
/// `cfg!(test)` and non-Windows builds.
pub fn show_windows_toast(title: &str, body: &str, launch_url: Option<&str>) -> bool {
    if cfg!(test) || (title.trim().is_empty() && body.trim().is_empty()) {
        return false;
    }

    #[cfg(windows)]
    {
        if let Ok(mut guard) = toast_rate_limiter().lock() {
            let now = Instant::now();
            if guard.is_some_and(|prev| now.duration_since(prev) < TOAST_MIN_INTERVAL) {
                return false;
            }
            *guard = Some(now);
        } else {
            return false;
        }

        let xml = build_toast_xml(title, body, launch_url);
        let app_id = FASTTUBE_APP_USER_MODEL_ID.to_owned();

        // Payload is passed via environment variables so untrusted video titles or
        // URLs are never interpolated into PowerShell script source.
        const PS_WINRT_TOAST_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[void][Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]
[void][Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime]
$doc = New-Object Windows.Data.Xml.Dom.XmlDocument
$doc.LoadXml($env:FASTTUBE_TOAST_XML)
$toast = New-Object Windows.UI.Notifications.ToastNotification $doc
try {
    [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($env:FASTTUBE_TOAST_APP_ID).Show($toast)
} catch {
    [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe').Show($toast)
}
"#;

        std::thread::Builder::new()
            .name("fasttube-win-toast".to_owned())
            .spawn(move || {
                use std::os::windows::process::CommandExt;
                use std::process::{Command, Stdio};

                let _ = Command::new("powershell.exe")
                    .args([
                        "-NoProfile",
                        "-NonInteractive",
                        "-WindowStyle",
                        "Hidden",
                        "-Command",
                        PS_WINRT_TOAST_SCRIPT,
                    ])
                    .env("FASTTUBE_TOAST_XML", xml)
                    .env("FASTTUBE_TOAST_APP_ID", app_id)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .creation_flags(CREATE_NO_WINDOW)
                    .status();
            })
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = (title, body, launch_url);
        false
    }
}

// ---------------------------------------------------------------------------
// 7. Portable HKCU `fasttube://` Protocol Registration (Zero Admin Required)
// ---------------------------------------------------------------------------

/// Builds the registry `shell\open\command` value `"<exe_path>" "%1"`.
pub fn build_protocol_open_command(exe_path: &Path) -> String {
    format!("\"{}\" \"%1\"", exe_path.display())
}

/// Checks whether `HKCU\Software\Classes\fasttube\shell\open\command` has a
/// non-empty default command registered.
pub fn is_fasttube_protocol_registered_hkcu() -> bool {
    #[cfg(windows)]
    {
        if cfg!(test) {
            return false;
        }
        let subkey = to_wide_null(r"Software\Classes\fasttube\shell\open\command");
        let mut size_bytes: u32 = 0;
        let status = unsafe {
            ffi::RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                ptr::null(),
                RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                ptr::null_mut(),
                ptr::null_mut(),
                &raw mut size_bytes,
            )
        };
        // A non-empty UTF-16 string requires more than a lone null terminator (2 bytes).
        status == ERROR_SUCCESS && size_bytes > 2
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn reg_set_sz(subkey: &str, value_name: Option<&str>, value: &str) -> Result<(), String> {
    let subkey_w = to_wide_null(subkey);
    let mut hkey: HKEY = ptr::null_mut();
    let create_status = unsafe {
        ffi::RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            0,
            ptr::null(),
            0,
            KEY_SET_VALUE | KEY_WRITE,
            ptr::null(),
            &raw mut hkey,
            ptr::null_mut(),
        )
    };
    if create_status != ERROR_SUCCESS || hkey.is_null() {
        return Err(format!(
            "registry key creation failed (code {create_status})"
        ));
    }
    let _guard = RegKeyGuard(hkey);

    let name_w = value_name.map(to_wide_null);
    let data_w = to_wide_null(value);
    let byte_len = (data_w.len() * std::mem::size_of::<u16>()) as u32;
    let set_status = unsafe {
        ffi::RegSetValueExW(
            hkey,
            name_w.as_ref().map_or(ptr::null(), |w| w.as_ptr()),
            0,
            REG_SZ,
            data_w.as_ptr().cast::<u8>(),
            byte_len,
        )
    };
    if set_status != ERROR_SUCCESS {
        return Err(format!("registry value write failed (code {set_status})"));
    }
    Ok(())
}

/// Registers the `fasttube://` URI handler under `HKCU\Software\Classes\fasttube`
/// without requiring Administrator privileges.
pub fn register_fasttube_protocol_hkcu(exe_path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        if cfg!(test) {
            return Ok(());
        }
        let icon_value = format!("\"{}\",0", exe_path.display());
        let open_cmd = build_protocol_open_command(exe_path);

        reg_set_sz(r"Software\Classes\fasttube", None, "URL:FastTube Protocol")?;
        reg_set_sz(r"Software\Classes\fasttube", Some("URL Protocol"), "")?;
        reg_set_sz(r"Software\Classes\fasttube\DefaultIcon", None, &icon_value)?;
        reg_set_sz(
            r"Software\Classes\fasttube\shell\open\command",
            None,
            &open_cmd,
        )?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = exe_path;
        Ok(())
    }
}

/// Unregisters the `fasttube://` URI handler from `HKCU\Software\Classes\fasttube`.
pub fn unregister_fasttube_protocol_hkcu() -> Result<(), String> {
    #[cfg(windows)]
    {
        if cfg!(test) {
            return Ok(());
        }
        let subkey_w = to_wide_null(r"Software\Classes\fasttube");
        let status = unsafe { ffi::RegDeleteTreeW(HKEY_CURRENT_USER, subkey_w.as_ptr()) };
        if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(format!("registry delete failed (code {status})"))
        }
    }
    #[cfg(not(windows))]
    {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Offline Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn com_vtable_slot_counts_and_thumbbutton_layout_match_win32_abi() {
        let ptr_size = size_of::<usize>();
        // IUnknown(3) + ITaskbarList(5) + ITaskbarList2(1) + ITaskbarList3(12) = 21 slots
        assert_eq!(size_of::<ITaskbarList3Vtbl>(), 21 * ptr_size);
        // IUnknown(3) + ICustomDestinationList(9) = 12 slots
        assert_eq!(size_of::<ICustomDestinationListVtbl>(), 12 * ptr_size);
        // IUnknown(3) + IObjectArray(2) + IObjectCollection(4) = 9 slots
        assert_eq!(size_of::<IObjectCollectionVtbl>(), 9 * ptr_size);
        // IUnknown(3) + IShellLinkW(18) = 21 slots
        assert_eq!(size_of::<IShellLinkWVtbl>(), 21 * ptr_size);
        // IUnknown(3) + IPropertyStore(5) = 8 slots
        assert_eq!(size_of::<IPropertyStoreVtbl>(), 8 * ptr_size);

        assert_eq!(size_of::<Guid>(), 16);
        assert_eq!(size_of::<PropertyKey>(), 20);
        assert_eq!(size_of::<PropVariant>(), 8 + 2 * ptr_size);

        // THUMBBUTTON uses natural alignment <= 8 (matching <pshpack8.h>):
        // 552 bytes on 64-bit Windows, 540 bytes on 32-bit Windows.
        assert_eq!(offset_of!(THUMBBUTTON, dw_mask), 0);
        assert_eq!(offset_of!(THUMBBUTTON, i_id), 4);
        assert_eq!(offset_of!(THUMBBUTTON, i_bitmap), 8);
        if ptr_size == 8 {
            assert_eq!(offset_of!(THUMBBUTTON, h_icon), 16);
            assert_eq!(offset_of!(THUMBBUTTON, sz_tip), 24);
            assert_eq!(offset_of!(THUMBBUTTON, dw_flags), 544);
            assert_eq!(size_of::<THUMBBUTTON>(), 552);
        } else {
            assert_eq!(offset_of!(THUMBBUTTON, h_icon), 12);
            assert_eq!(offset_of!(THUMBBUTTON, sz_tip), 16);
            assert_eq!(offset_of!(THUMBBUTTON, dw_flags), 536);
            assert_eq!(size_of::<THUMBBUTTON>(), 540);
        }
    }

    #[test]
    fn snap_layout_rect_contains_handles_bounds_and_degenerate_rects() {
        let rect = SnapLayoutRect::new(1200, 0, 1246, 32);
        assert!(rect.is_valid());
        assert!(rect.contains(1200, 0));
        assert!(rect.contains(1223, 16));
        assert!(rect.contains(1245, 31));
        assert!(!rect.contains(1199, 16));
        assert!(!rect.contains(1246, 16));
        assert!(!rect.contains(1220, -1));
        assert!(!rect.contains(1220, 32));

        let empty = SnapLayoutRect::new(100, 10, 100, 40);
        assert!(!empty.is_valid());
        assert!(!empty.contains(100, 20));

        let inverted = SnapLayoutRect::new(200, 50, 100, 10);
        assert!(!inverted.is_valid());
        assert!(!inverted.contains(150, 30));
    }

    #[test]
    fn build_toast_xml_escapes_special_characters_and_control_bytes() {
        let xml = build_toast_xml(
            "A & B <Live> \"Now\" '1'\x07",
            "Watch <script>&\"test\"'</script>",
            Some("fasttube://watch?v=abc&t=10\"'<>"),
        );
        assert!(
            xml.starts_with(
                "<toast launch=\"fasttube://watch?v=abc&amp;t=10&quot;&apos;&lt;&gt;\">"
            )
        );
        assert!(xml.contains("<text>A &amp; B &lt;Live&gt; &quot;Now&quot; &apos;1&apos;</text>"));
        assert!(xml.contains(
            "<text>Watch &lt;script&gt;&amp;&quot;test&quot;&apos;&lt;/script&gt;</text>"
        ));
        assert!(!xml.contains('\x07'));

        let no_launch = build_toast_xml("Title", "Body", None);
        assert!(no_launch.starts_with("<toast><visual>"));
        assert!(!no_launch.contains("launch="));
    }

    #[test]
    fn default_jump_list_tasks_are_bilingual_and_use_section_uris() {
        let pt_tasks = default_jump_list_tasks(true);
        let en_tasks = default_jump_list_tasks(false);
        assert_eq!(pt_tasks.len(), 5);
        assert_eq!(en_tasks.len(), 5);

        let expected_args = [
            "fasttube://section/home",
            "fasttube://section/subscriptions",
            "fasttube://section/history",
            "fasttube://section/downloads",
            "fasttube://section/watch_later",
        ];
        for (i, expected) in expected_args.iter().enumerate() {
            assert_eq!(pt_tasks[i].args, *expected);
            assert_eq!(en_tasks[i].args, *expected);
            assert!(!pt_tasks[i].title.is_empty());
            assert!(!en_tasks[i].title.is_empty());
            assert!(!pt_tasks[i].description.is_empty());
            assert!(!en_tasks[i].description.is_empty());
        }
        assert_eq!(pt_tasks[0].title, "Início");
        assert_eq!(en_tasks[0].title, "Home");
        assert_eq!(pt_tasks[4].title, "Assistir mais tarde");
        assert_eq!(en_tasks[4].title, "Watch Later");
    }

    #[test]
    fn build_protocol_open_command_quotes_executable_and_placeholder() {
        let cmd = build_protocol_open_command(Path::new(r"C:\Program Files\FastTube\fasttube.exe"));
        assert_eq!(cmd, r#""C:\Program Files\FastTube\fasttube.exe" "%1""#);
    }
}
