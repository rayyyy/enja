//! Global keyboard listener (macOS).
//!
//! Uses CGEventTap directly instead of the `rdev` crate. rdev internally calls
//! TISGetInputSourceProperty (Text Services Manager) from the event-tap thread
//! to resolve key names. On macOS Sequoia+ Apple added a dispatch_assert_queue
//! assertion requiring those TSM calls to happen on the main thread, causing an
//! instant SIGTRAP crash. Enja only needs a small set of raw key codes and
//! modifier flags, so we skip TSM entirely and work with CGEvent directly.

#[derive(Debug, Clone, Copy)]
pub enum KeyboardTrigger {
    CmdCopyDouble,
    FunctionPress,
    FunctionRelease,
    FunctionSpace,
    Escape,
}

#[cfg(target_os = "macos")]
mod macos {
    use super::KeyboardTrigger;
    use std::os::raw::c_void;
    use std::sync::mpsc::Sender;
    use std::time::{Duration, Instant};

    // --- FFI types -----------------------------------------------------------

    type CGEventRef = *mut c_void;
    type CFMachPortRef = *const c_void;
    type CFRunLoopSourceRef = *const c_void;
    type CFRunLoopRef = *const c_void;
    type CFRunLoopMode = *const c_void;
    type CFAllocatorRef = *const c_void;

    type CGEventTapCallBack = unsafe extern "C" fn(
        proxy: *const c_void,
        event_type: u32,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef;

    // --- Constants -----------------------------------------------------------

    const KCG_HID_EVENT_TAP: u32 = 0;
    const KCG_HEAD_INSERT_EVENT_TAP: u32 = 0;
    const KCG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;

    const KCG_EVENT_KEY_DOWN: u32 = 10;
    const KCG_EVENT_KEY_UP: u32 = 11;
    const KCG_EVENT_FLAGS_CHANGED: u32 = 12;
    const KCG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const KCG_EVENT_TAP_DISABLED_BY_USER: u32 = 0xFFFF_FFFF;

    const KCG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
    const KCG_EVENT_FLAG_MASK_COMMAND: u64 = 0x0010_0000;
    const KCG_EVENT_FLAG_MASK_SECONDARY_FN: u64 = 0x0080_0000;

    /// KeyDown | KeyUp | FlagsChanged
    const EVENT_MASK: u64 = (1 << 10) | (1 << 11) | (1 << 12);

    const KEYCODE_C: i64 = 8;
    const KEYCODE_SPACE: i64 = 49;
    const KEYCODE_ESCAPE: i64 = 53;

    // --- FFI bindings --------------------------------------------------------

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
        fn CGEventGetFlags(event: CGEventRef) -> u64;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFMachPortCreateRunLoopSource(
            allocator: CFAllocatorRef,
            port: CFMachPortRef,
            order: i64,
        ) -> CFRunLoopSourceRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFRunLoopMode);
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopRun();
        static kCFRunLoopCommonModes: CFRunLoopMode;
    }

    // --- Listener state (single-threaded: only accessed from the tap thread) -

    use std::sync::Mutex;

    struct ListenerState {
        tx: Sender<KeyboardTrigger>,
        threshold: Duration,
        meta_down: bool,
        fn_down: bool,
        fn_space_combo: bool,
        fn_space_down: bool,
        last_cmd_c: Option<Instant>,
        tap: CFMachPortRef,
    }

    // Safety: CFMachPortRef is only used from the tap thread's CFRunLoop.
    unsafe impl Send for ListenerState {}

    static LISTENER_STATE: Mutex<Option<Box<ListenerState>>> = Mutex::new(None);

    // --- Event tap callback --------------------------------------------------

    unsafe extern "C" fn raw_callback(
        _proxy: *const c_void,
        event_type: u32,
        event: CGEventRef,
        _user_info: *mut c_void,
    ) -> CGEventRef {
        if event_type == KCG_EVENT_TAP_DISABLED_BY_TIMEOUT
            || event_type == KCG_EVENT_TAP_DISABLED_BY_USER
        {
            eprintln!("[enja] event tap disabled (type={event_type:#X}) — re-enabling");
            if let Ok(guard) = LISTENER_STATE.lock() {
                if let Some(state) = guard.as_ref() {
                    CGEventTapEnable(state.tap, true);
                }
            }
            return event;
        }

        if event.is_null() {
            return event;
        }

        let Ok(mut guard) = LISTENER_STATE.lock() else {
            return event;
        };
        let Some(state) = guard.as_mut() else {
            return event;
        };

        match event_type {
            KCG_EVENT_FLAGS_CHANGED => {
                let flags = CGEventGetFlags(event);
                let cmd_down = (flags & KCG_EVENT_FLAG_MASK_COMMAND) != 0;
                let fn_down = (flags & KCG_EVENT_FLAG_MASK_SECONDARY_FN) != 0;
                if cmd_down != state.meta_down {
                    eprintln!(
                        "[enja] meta_down: {} → {} (flags={flags:#X})",
                        state.meta_down, cmd_down
                    );
                    state.meta_down = cmd_down;
                }
                if fn_down != state.fn_down {
                    eprintln!(
                        "[enja] fn_down: {} → {} (flags={flags:#X})",
                        state.fn_down, fn_down
                    );
                    state.fn_down = fn_down;
                    if fn_down {
                        handle_fn_pressed(state);
                    } else {
                        handle_fn_released(state);
                    }
                }
            }
            KCG_EVENT_KEY_DOWN => {
                let keycode = CGEventGetIntegerValueField(event, KCG_KEYBOARD_EVENT_KEYCODE);
                if keycode == KEYCODE_ESCAPE {
                    let _ = state.tx.send(KeyboardTrigger::Escape);
                    return event;
                }
                if keycode == KEYCODE_SPACE && state.fn_down {
                    if !state.fn_space_down {
                        let _ = state.tx.send(KeyboardTrigger::FunctionSpace);
                    }
                    state.fn_space_down = true;
                    state.fn_space_combo = true;
                    return std::ptr::null_mut();
                }
                if state.meta_down {
                    eprintln!("[enja] KeyDown while Cmd held: keycode={keycode}");
                }
                if keycode == KEYCODE_C && state.meta_down {
                    let now = Instant::now();
                    if let Some(prev) = state.last_cmd_c {
                        let elapsed = now.duration_since(prev);
                        eprintln!(
                            "[enja] Cmd+C (2nd) interval={:?} threshold={:?}",
                            elapsed, state.threshold
                        );
                        if elapsed <= state.threshold {
                            eprintln!("[enja] >>> TRIGGER!");
                            let _ = state.tx.send(KeyboardTrigger::CmdCopyDouble);
                            state.last_cmd_c = None;
                        } else {
                            state.last_cmd_c = Some(now);
                        }
                    } else {
                        eprintln!("[enja] Cmd+C (1st)");
                        state.last_cmd_c = Some(now);
                    }
                }
            }
            KCG_EVENT_KEY_UP => {
                let keycode = CGEventGetIntegerValueField(event, KCG_KEYBOARD_EVENT_KEYCODE);
                if keycode == KEYCODE_SPACE && state.fn_space_down {
                    state.fn_space_down = false;
                    return std::ptr::null_mut();
                }
            }
            _ => {}
        }

        event
    }

    fn handle_fn_pressed(state: &mut ListenerState) {
        // Any new Fn press starts a fresh combo cycle.
        state.fn_space_combo = false;
        state.fn_space_down = false;
        let _ = state.tx.send(KeyboardTrigger::FunctionPress);
    }

    fn handle_fn_released(state: &mut ListenerState) {
        if state.fn_space_combo {
            // The matching Fn down formed a chord with Space — Ask was triggered
            // separately, so this release should not produce a Function event.
            state.fn_space_combo = false;
            return;
        }
        let _ = state.tx.send(KeyboardTrigger::FunctionRelease);
    }

    // --- Public entry point --------------------------------------------------

    pub fn spawn_listener(tx: Sender<KeyboardTrigger>, threshold_ms: u64) {
        let threshold = Duration::from_millis(threshold_ms.max(50));

        std::thread::spawn(move || unsafe {
            let tap = CGEventTapCreate(
                KCG_HID_EVENT_TAP,
                KCG_HEAD_INSERT_EVENT_TAP,
                KCG_EVENT_TAP_OPTION_DEFAULT,
                EVENT_MASK,
                raw_callback,
                std::ptr::null_mut(),
            );
            if tap.is_null() {
                eprintln!(
                    "[enja] CGEventTapCreate failed. \
                         Grant Input Monitoring / Accessibility permission for this app."
                );
                return;
            }
            eprintln!("[enja] CGEventTap created successfully");

            if let Ok(mut guard) = LISTENER_STATE.lock() {
                *guard = Some(Box::new(ListenerState {
                    tx,
                    threshold,
                    meta_down: false,
                    fn_down: false,
                    fn_space_combo: false,
                    fn_space_down: false,
                    last_cmd_c: None,
                    tap,
                }));
            }

            let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            if source.is_null() {
                eprintln!("[enja] CFMachPortCreateRunLoopSource failed");
                return;
            }

            let current_loop = CFRunLoopGetCurrent();
            CFRunLoopAddSource(current_loop, source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            eprintln!(
                "[enja] keyboard listener running (threshold={}ms)",
                threshold.as_millis()
            );
            CFRunLoopRun();
            eprintln!("[enja] keyboard listener CFRunLoop exited");
        });
    }
}

#[cfg(target_os = "macos")]
pub use macos::spawn_listener;

#[cfg(not(target_os = "macos"))]
pub fn spawn_listener(_tx: std::sync::mpsc::Sender<KeyboardTrigger>, _threshold_ms: u64) {}
