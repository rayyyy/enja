//! 音声オーバーレイウィンドウの表示・配置・カーソル追従。

#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) const VOICE_WINDOW_EDGE_MARGIN: f64 = 16.0;

pub(crate) const VOICE_WINDOW_BOTTOM_MARGIN: f64 = 42.0;

pub(crate) const VOICE_WINDOW_FOLLOW_INTERVAL_MS: u64 = 180;

pub(crate) static VOICE_WINDOW_FOLLOW_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VoiceWindowMonitorKey {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scale_bits: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoiceWindowLayout {
    Compact,
    Expanded,
    Notice,
    CheatSheet,
}

impl VoiceWindowLayout {
    fn dimensions(self) -> (f64, f64) {
        match self {
            Self::Compact => (292.0, 42.0),
            Self::Expanded => (840.0, 420.0),
            Self::Notice => (460.0, 64.0),
            Self::CheatSheet => (480.0, 162.0),
        }
    }

    fn min_height(self) -> f64 {
        match self {
            Self::Expanded => 260.0,
            Self::Notice => 58.0,
            Self::CheatSheet => 148.0,
            Self::Compact => 40.0,
        }
    }

    fn focusable(self) -> bool {
        matches!(self, Self::Expanded | Self::Notice)
    }
}

pub fn show_shortcut_cheat_sheet(app: &tauri::AppHandle) {
    show_voice_window_with_layout(app, VoiceWindowLayout::CheatSheet);
    emit_state(app, "cheatSheet", None, None, None);
}

pub fn hide_shortcut_cheat_sheet(app: &tauri::AppHandle) {
    emit_state(app, "idle", None, None, None);
    hide_voice_window(app);
}

pub(crate) fn show_voice_window(app: &tauri::AppHandle, expanded: bool) {
    let layout = if expanded {
        VoiceWindowLayout::Expanded
    } else {
        VoiceWindowLayout::Compact
    };
    show_voice_window_with_layout(app, layout);
}

pub(crate) fn show_voice_notice_window(app: &tauri::AppHandle) {
    show_voice_window_with_layout(app, VoiceWindowLayout::Notice);
}

pub(crate) fn show_voice_window_with_layout(app: &tauri::AppHandle, layout: VoiceWindowLayout) {
    let Some(window) = app.get_webview_window("voice") else {
        crate::keyboard::set_voice_overlay_visible(false);
        return;
    };
    let monitor_key = configure_voice_window(app, &window, layout);
    let _ = window.set_always_on_top(true);
    if window.show().is_ok() {
        crate::keyboard::set_voice_overlay_visible(true);
    }
    start_voice_window_follow(app, layout, monitor_key);
}

pub(crate) fn configure_voice_window(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    layout: VoiceWindowLayout,
) -> Option<VoiceWindowMonitorKey> {
    let target_monitor = voice_window_target_monitor(app);
    let monitor_key = target_monitor.as_ref().map(voice_window_monitor_key);
    let scale = target_monitor
        .as_ref()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or_else(|| window.scale_factor().unwrap_or(1.0))
        .max(1.0);
    let (mut width, mut height) = layout.dimensions();
    if let Some(monitor) = target_monitor.as_ref() {
        let size = monitor.size();
        let logical_width = size.width as f64 / scale;
        let logical_height = size.height as f64 / scale;
        width = width.min((logical_width - 40.0).max(260.0));
        height = height.min((logical_height - 88.0).max(layout.min_height()));
    }
    let _ = window.set_focusable(layout.focusable());
    let _ = window.set_shadow(layout.focusable());
    let _ = window.set_size(tauri::LogicalSize::new(width, height));
    if let Some(monitor) = target_monitor.as_ref() {
        let pos = monitor.position();
        let size = monitor.size();
        let screen_pos = pos.to_logical::<f64>(scale);
        let screen_size = size.to_logical::<f64>(scale);
        let screen_x = screen_pos.x;
        let screen_y = screen_pos.y;
        let screen_width = screen_size.width;
        let screen_height = screen_size.height;
        let edge_margin = VOICE_WINDOW_EDGE_MARGIN;
        let bottom_margin = VOICE_WINDOW_BOTTOM_MARGIN;
        let window_width = width;
        let window_height = height;
        let min_x = screen_x + edge_margin;
        let max_x = (screen_x + screen_width - window_width - edge_margin).max(min_x);
        let x = (screen_x + (screen_width - window_width) / 2.0).clamp(min_x, max_x);
        let min_y = screen_y + edge_margin;
        let max_y = (screen_y + screen_height - window_height - edge_margin).max(min_y);
        let y = (screen_y + screen_height - window_height - bottom_margin).clamp(min_y, max_y);
        let _ = window.set_position(tauri::LogicalPosition::new(x, y));
    }

    monitor_key
}

pub(crate) fn start_voice_window_follow(
    app: &tauri::AppHandle,
    layout: VoiceWindowLayout,
    monitor_key: Option<VoiceWindowMonitorKey>,
) {
    let token = VOICE_WINDOW_FOLLOW_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut current_monitor = monitor_key;
        loop {
            tokio::time::sleep(Duration::from_millis(VOICE_WINDOW_FOLLOW_INTERVAL_MS)).await;
            if VOICE_WINDOW_FOLLOW_SEQ.load(Ordering::SeqCst) != token {
                return;
            }

            let Some(window) = app.get_webview_window("voice") else {
                return;
            };
            if !window.is_visible().unwrap_or(false) {
                return;
            }

            let next_monitor = voice_window_target_monitor(&app)
                .as_ref()
                .map(voice_window_monitor_key);
            if next_monitor != current_monitor {
                current_monitor = configure_voice_window(&app, &window, layout);
            }
        }
    });
}

pub(crate) fn stop_voice_window_follow() {
    VOICE_WINDOW_FOLLOW_SEQ.fetch_add(1, Ordering::SeqCst);
}

pub(crate) fn voice_window_target_monitor(
    app: &tauri::AppHandle,
) -> Option<tauri::window::Monitor> {
    if let Ok(cursor) = app.cursor_position() {
        if let Ok(Some(monitor)) = app.monitor_from_point(cursor.x, cursor.y) {
            return Some(monitor);
        }

        if let Ok(monitors) = app.available_monitors() {
            if let Some(monitor) = monitors
                .into_iter()
                .find(|monitor| monitor_contains_physical_point(monitor, cursor.x, cursor.y))
            {
                return Some(monitor);
            }
        }
    }

    app.primary_monitor().ok().flatten()
}

pub(crate) fn voice_window_monitor_key(monitor: &tauri::window::Monitor) -> VoiceWindowMonitorKey {
    let pos = monitor.position();
    let size = monitor.size();
    VoiceWindowMonitorKey {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
        scale_bits: monitor.scale_factor().to_bits(),
    }
}

pub(crate) fn monitor_contains_physical_point(
    monitor: &tauri::window::Monitor,
    x: f64,
    y: f64,
) -> bool {
    let pos = monitor.position();
    let size = monitor.size();
    let left = pos.x as f64;
    let top = pos.y as f64;
    x >= left && x < left + size.width as f64 && y >= top && y < top + size.height as f64
}

pub(crate) fn hide_voice_window(app: &tauri::AppHandle) {
    stop_voice_window_follow();
    crate::keyboard::set_voice_overlay_visible(false);
    if let Some(window) = app.get_webview_window("voice") {
        let _ = window.hide();
    }
}

pub(crate) fn hide_voice_window_after(app: tauri::AppHandle, delay: Duration) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        hide_voice_window(&app);
    });
}
