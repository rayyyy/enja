//! macOS: Show main window on Cmd+Tab when it was hidden via `hide()`.
//! Dock uses `RunEvent::Reopen`; switching apps does not, so listen for `NSApplicationDidBecomeActive`.

#[cfg(target_os = "macos")]
mod imp {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplicationDidBecomeActiveNotification;
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue};
    use std::ptr::NonNull;
    use std::sync::OnceLock;
    use tauri::{AppHandle, Manager};

    static HANDLE: OnceLock<AppHandle> = OnceLock::new();

    pub fn init(handle: AppHandle) {
        let _ = HANDLE.set(handle);

        if MainThreadMarker::new().is_none() {
            return;
        }

        unsafe {
            let center = NSNotificationCenter::defaultCenter();
            let queue = NSOperationQueue::mainQueue();
            let block = RcBlock::new(|_n: NonNull<NSNotification>| {
                show_main_if_hidden();
            });
            let observer = center.addObserverForName_object_queue_usingBlock(
                Some(NSApplicationDidBecomeActiveNotification),
                None,
                Some(&queue),
                &block,
            );
            std::mem::forget(observer);
        }
    }

    fn show_main_if_hidden() {
        let Some(h) = HANDLE.get() else {
            return;
        };
        if has_visible_auxiliary_window(h) {
            return;
        }
        if let Some(w) = h.get_webview_window("main") {
            if let Ok(false) = w.is_visible() {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }
    }

    fn has_visible_auxiliary_window(h: &AppHandle) -> bool {
        h.webview_windows().into_iter().any(|(label, window)| {
            is_auxiliary_window(&label) && window.is_visible().unwrap_or(false)
        })
    }

    fn is_auxiliary_window(label: &str) -> bool {
        label == "voice" || label.starts_with("sticky-")
    }
}

#[cfg(target_os = "macos")]
pub use imp::init;

#[cfg(not(target_os = "macos"))]
pub fn init(_handle: tauri::AppHandle) {}
