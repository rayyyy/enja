//! CoreGraphics / CoreFoundation の FFI 宣言。

#[allow(clippy::wildcard_imports)]
use super::*;

// --- FFI types -----------------------------------------------------------

pub(crate) type CGEventRef = *mut c_void;
pub(crate) type CFMachPortRef = *const c_void;
pub(crate) type CFRunLoopSourceRef = *const c_void;
pub(crate) type CFRunLoopRef = *const c_void;
pub(crate) type CFRunLoopMode = *const c_void;
pub(crate) type CFAllocatorRef = *const c_void;

pub(crate) type CGEventTapCallBack = unsafe extern "C" fn(
    proxy: *const c_void,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef;

// --- Constants -----------------------------------------------------------

pub(crate) const KCG_HID_EVENT_TAP: u32 = 0;
pub(crate) const KCG_HEAD_INSERT_EVENT_TAP: u32 = 0;
pub(crate) const KCG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;

pub(crate) const KCG_EVENT_KEY_DOWN: u32 = 10;
pub(crate) const KCG_EVENT_KEY_UP: u32 = 11;
pub(crate) const KCG_EVENT_FLAGS_CHANGED: u32 = 12;
pub(crate) const KCG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
pub(crate) const KCG_EVENT_TAP_DISABLED_BY_USER: u32 = 0xFFFF_FFFF;

pub(crate) const KCG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
pub(crate) const KCG_EVENT_FLAG_MASK_SHIFT: u64 = 0x0002_0000;
pub(crate) const KCG_EVENT_FLAG_MASK_CONTROL: u64 = 0x0004_0000;
pub(crate) const KCG_EVENT_FLAG_MASK_ALTERNATE: u64 = 0x0008_0000;
pub(crate) const KCG_EVENT_FLAG_MASK_COMMAND: u64 = 0x0010_0000;
pub(crate) const KCG_EVENT_FLAG_MASK_SECONDARY_FN: u64 = 0x0080_0000;

/// KeyDown | KeyUp | FlagsChanged
pub(crate) const EVENT_MASK: u64 = (1 << 10) | (1 << 11) | (1 << 12);

// --- FFI bindings --------------------------------------------------------

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub(crate) fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    pub(crate) fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    pub(crate) fn CGEventGetFlags(event: CGEventRef) -> u64;
    pub(crate) fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub(crate) fn CFMachPortCreateRunLoopSource(
        allocator: CFAllocatorRef,
        port: CFMachPortRef,
        order: i64,
    ) -> CFRunLoopSourceRef;
    pub(crate) fn CFRunLoopAddSource(
        rl: CFRunLoopRef,
        source: CFRunLoopSourceRef,
        mode: CFRunLoopMode,
    );
    pub(crate) fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    pub(crate) fn CFRunLoopRun();
    pub(crate) static kCFRunLoopCommonModes: CFRunLoopMode;
}
