//! ペースト先の解決・AX 読み取り・クリップボード貼り付けと検証。

#[allow(clippy::wildcard_imports)]
use super::*;

#[cfg(target_os = "macos")]
pub(crate) const PASTE_WRITE_SETTLE_MS: u64 = 40;

/// Cmd+V 後の挿入検証ポーリング間隔。
#[cfg(target_os = "macos")]
pub(crate) const PASTE_VERIFY_POLL_MS: u64 = 40;

/// 挿入検証を諦めるまでの最大待ち時間。検証成功または このタイムアウトまで
/// クリップボードを復元しない(遅いアプリが復元後に元の内容を貼る競合を防ぐ)。
#[cfg(target_os = "macos")]
pub(crate) const PASTE_VERIFY_TIMEOUT_MS: u64 = 600;

/// Cmd+V 送信前のスナップショット取得を再試行するまでの待ち時間。
#[cfg(target_os = "macos")]
pub(crate) const PASTE_SNAPSHOT_RETRY_DELAY_MS: u64 = 80;

#[cfg(target_os = "macos")]
pub(crate) const PASTE_ACTIVATE_SETTLE_MS: u64 = 80;

#[cfg(target_os = "macos")]
pub(crate) const MANUAL_ACCESSIBILITY_POLL_ATTEMPTS: usize = 10;

#[cfg(target_os = "macos")]
pub(crate) const MANUAL_ACCESSIBILITY_POLL_INTERVAL: Duration = Duration::from_millis(30);

#[cfg(target_os = "macos")]
pub(crate) const MANUAL_ACCESSIBILITY_FAILURE_TTL: Duration = Duration::from_secs(2);

#[cfg(target_os = "macos")]
pub(crate) static MANUAL_ACCESSIBILITY_CACHE: OnceLock<Mutex<ManualAccessibilityCache>> =
    OnceLock::new();

#[derive(Debug, Clone)]
pub(crate) struct PasteTargetInfo {
    pub(crate) pid: Option<i32>,
    pub(crate) role: String,
    pub(crate) subrole: String,
    pub(crate) attributes: HashSet<String>,
}

impl PasteTargetInfo {
    fn from_osascript_output(output: &str) -> Option<Self> {
        if output.trim().is_empty() {
            return None;
        }

        let lines = output.lines().collect::<Vec<_>>();
        let first = lines.first().copied().unwrap_or_default().trim();
        let (pid, role_index) = match first.parse::<i32>() {
            Ok(pid) if pid > 0 => (Some(pid), 1),
            _ => (None, 0),
        };
        let role = lines
            .get(role_index)
            .copied()
            .unwrap_or_default()
            .trim()
            .to_string();
        let subrole = lines
            .get(role_index + 1)
            .copied()
            .unwrap_or_default()
            .trim()
            .to_string();
        let attributes = lines
            .get(role_index + 2)
            .copied()
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect::<HashSet<_>>();

        if pid.is_none() && role.is_empty() && subrole.is_empty() && attributes.is_empty() {
            return None;
        }

        Some(Self {
            pid,
            role,
            subrole,
            attributes,
        })
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Default)]
pub(crate) struct ManualAccessibilityCache {
    pub(crate) enabled_pids: HashSet<i32>,
    pub(crate) failed_until_by_pid: HashMap<i32, Instant>,
}

#[cfg(target_os = "macos")]
pub(crate) fn capture_paste_target() -> Option<PasteTargetInfo> {
    current_paste_target_info()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn capture_paste_target() -> Option<PasteTargetInfo> {
    None
}

#[cfg(target_os = "macos")]
pub(crate) fn capture_selected_text() -> String {
    if let Some(selected) = read_accessibility_selected_text() {
        return selected;
    }

    let original = read_clipboard_text();
    let sentinel = new_clipboard_sentinel();
    if !write_clipboard_text(&sentinel) {
        return String::new();
    }

    if !run_keystroke("c") {
        restore_clipboard(original);
        return String::new();
    }

    let selected =
        wait_for_copied_selection(&sentinel, Duration::from_millis(700)).unwrap_or_default();
    restore_clipboard(original);
    selected
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn capture_selected_text() -> String {
    String::new()
}

#[cfg(target_os = "macos")]
pub(crate) fn read_accessibility_selected_text() -> Option<String> {
    let script = r#"
tell application "System Events"
  try
    set frontApp to first application process whose frontmost is true
    set focusedElement to value of attribute "AXFocusedUIElement" of frontApp
    set selectedText to value of attribute "AXSelectedText" of focusedElement
    if selectedText is missing value then return ""
    return selectedText as text
  on error
    return ""
  end try
end tell
"#;
    let output = std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn new_clipboard_sentinel() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("__ENJA_SELECTED_TEXT_SENTINEL_{nanos}__")
}

#[cfg(target_os = "macos")]
pub(crate) fn wait_for_copied_selection(sentinel: &str, timeout: Duration) -> Option<String> {
    let start = Instant::now();
    loop {
        if let Some(value) = read_clipboard_text() {
            if value != sentinel {
                return Some(value);
            }
        }
        if start.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "macos")]
pub(crate) type AXUIElementRef = *const c_void;

#[cfg(target_os = "macos")]
pub(crate) type AXValueRef = *const c_void;

#[cfg(target_os = "macos")]
pub(crate) type AXError = c_int;

#[cfg(target_os = "macos")]
pub(crate) type Boolean = u8;

#[cfg(target_os = "macos")]
pub(crate) const KAX_ERROR_SUCCESS: AXError = 0;

#[cfg(target_os = "macos")]
pub(crate) const KAX_VALUE_CF_RANGE_TYPE: c_int = 4;

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct AxCfRange {
    pub(crate) location: isize,
    pub(crate) length: isize,
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    pub(crate) fn AXUIElementCreateApplication(pid: c_int) -> AXUIElementRef;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementCopyAttributeNames(element: AXUIElementRef, names: *mut CFArrayRef) -> AXError;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut c_int) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXValueGetType(value: AXValueRef) -> c_int;
    fn AXValueGetValue(value: AXValueRef, value_type: c_int, value_ptr: *mut c_void) -> Boolean;
}

#[cfg(target_os = "macos")]
pub(crate) struct AxElementRef {
    pub(crate) raw: AXUIElementRef,
}

#[cfg(target_os = "macos")]
unsafe impl Send for AxElementRef {}

#[cfg(target_os = "macos")]
impl Drop for AxElementRef {
    fn drop(&mut self) {
        unsafe {
            if !self.raw.is_null() {
                CFRelease(self.raw.cast());
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
pub(crate) struct AxTextSnapshot {
    pub(crate) pid: c_int,
    pub(crate) value: String,
    pub(crate) selected_range: TextRange,
}

#[cfg(target_os = "macos")]
pub(crate) struct AxFocusedElement {
    pub(crate) element: AxElementRef,
}

#[cfg(target_os = "macos")]
impl AxFocusedElement {
    pub(crate) fn capture() -> Option<Self> {
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return None;
            }
            let focused =
                copy_ax_attribute_raw(system, "AXFocusedUIElement").map(|raw| AxElementRef {
                    raw: raw as AXUIElementRef,
                });
            CFRelease(system.cast());

            focused.map(|element| Self { element })
        }
    }

    fn read_paste_target_info(&self) -> Option<PasteTargetInfo> {
        let pid = self.element.pid()?;
        Some(PasteTargetInfo {
            pid: Some(pid),
            role: copy_ax_string_attribute(self.element.raw, "AXRole").unwrap_or_default(),
            subrole: copy_ax_string_attribute(self.element.raw, "AXSubrole").unwrap_or_default(),
            attributes: copy_ax_attribute_names(self.element.raw).unwrap_or_default(),
        })
    }
}

#[cfg(target_os = "macos")]
pub(crate) struct AxFocusedText {
    pub(crate) element: AxElementRef,
    pub(crate) snapshot: AxTextSnapshot,
}

#[cfg(target_os = "macos")]
pub(crate) struct VerifiedPaste {
    pub(crate) target: AxFocusedText,
    pub(crate) after_paste: AxTextSnapshot,
    pub(crate) insertion: VerifiedPasteInsertion,
}

#[cfg(target_os = "macos")]
pub(crate) enum VerifiedPasteInsertion {
    Changed(TextRange),
    SameTextReplacement,
}

#[cfg(target_os = "macos")]
impl AxFocusedText {
    pub(crate) fn capture() -> Option<Self> {
        let focused = AxFocusedElement::capture()?;
        let snapshot = focused.element.read_text_snapshot()?;
        Some(Self {
            element: focused.element,
            snapshot,
        })
    }

    pub(crate) fn capture_for_paste_target(target: &PasteTargetInfo) -> Option<Self> {
        let focused = Self::capture()?;
        if target.pid.is_some_and(|pid| focused.snapshot.pid != pid) {
            return None;
        }
        Some(focused)
    }
}

#[cfg(target_os = "macos")]
impl AxElementRef {
    fn pid(&self) -> Option<c_int> {
        if self.raw.is_null() {
            return None;
        }
        let mut pid: c_int = 0;
        unsafe {
            if AXUIElementGetPid(self.raw, &mut pid) != KAX_ERROR_SUCCESS {
                return None;
            }
        }
        Some(pid)
    }

    pub(crate) fn read_text_snapshot(&self) -> Option<AxTextSnapshot> {
        let pid = self.pid()?;
        let raw_value = copy_ax_string_attribute(self.raw, "AXValue")?;
        let placeholder = copy_ax_string_attribute(self.raw, "AXPlaceholderValue");
        let value = value_without_placeholder(raw_value, placeholder.as_deref());
        let selected_range = copy_ax_range_attribute(self.raw, "AXSelectedTextRange")?;
        Some(AxTextSnapshot {
            pid,
            value,
            selected_range,
        })
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn copy_ax_attribute_raw(element: AXUIElementRef, attribute: &str) -> Option<CFTypeRef> {
    let attribute = CFString::new(attribute);
    let mut value: CFTypeRef = std::ptr::null();
    let status = unsafe {
        AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value)
    };
    if status == KAX_ERROR_SUCCESS && !value.is_null() {
        Some(value)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn copy_ax_string_attribute(element: AXUIElementRef, attribute: &str) -> Option<String> {
    let value = copy_ax_attribute_raw(element, attribute)?;
    unsafe {
        if CFGetTypeID(value) != CFStringGetTypeID() {
            CFRelease(value);
            return None;
        }
        let text = CFString::wrap_under_create_rule(value as CFStringRef).to_string();
        Some(text)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn copy_ax_range_attribute(
    element: AXUIElementRef,
    attribute: &str,
) -> Option<TextRange> {
    let value = copy_ax_attribute_raw(element, attribute)?;
    unsafe {
        if AXValueGetType(value as AXValueRef) != KAX_VALUE_CF_RANGE_TYPE {
            CFRelease(value);
            return None;
        }
        let mut range = AxCfRange {
            location: 0,
            length: 0,
        };
        let ok = AXValueGetValue(
            value as AXValueRef,
            KAX_VALUE_CF_RANGE_TYPE,
            &mut range as *mut _ as *mut c_void,
        ) != 0;
        CFRelease(value);
        if !ok || range.location < 0 || range.length < 0 {
            return None;
        }
        Some(TextRange {
            location: range.location as usize,
            length: range.length as usize,
        })
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn copy_ax_attribute_names(element: AXUIElementRef) -> Option<HashSet<String>> {
    let mut names: CFArrayRef = std::ptr::null();
    let status = unsafe { AXUIElementCopyAttributeNames(element, &mut names) };
    if status != KAX_ERROR_SUCCESS || names.is_null() {
        return None;
    }

    let mut out = HashSet::new();
    unsafe {
        let count = CFArrayGetCount(names);
        for index in 0..count {
            let value = CFArrayGetValueAtIndex(names, index);
            if !value.is_null() && CFGetTypeID(value.cast()) == CFStringGetTypeID() {
                let text = CFString::wrap_under_get_rule(value as CFStringRef).to_string();
                out.insert(text);
            }
        }
        CFRelease(names.cast());
    }

    Some(out)
}

/// 貼り付け試行の結果。
/// - `Verified`: AX でテキストの変化を確認できた(辞書学習に使える)。
/// - `Unverified`: Cmd+V は対象アプリに届いたはずだが、AX では挿入を確認
///   できなかった(Electron/Monaco や WKWebView は AX に変化が出ないことが
///   ある)。挿入済みとして扱うが、誤検出の恐れがあるため学習はしない。
/// - `Failed`: 入力先を解決できない・キー送信に失敗・貼り付け中にフォーカス
///   が別アプリへ移った。フォールバック表示で本文を救済する。
#[cfg(target_os = "macos")]
pub(crate) enum PasteAttempt {
    Verified(Box<VerifiedPaste>),
    Unverified,
    Failed,
}

#[cfg(target_os = "macos")]
pub(crate) fn paste_text(text: &str, preferred_target: Option<&PasteTargetInfo>) -> bool {
    !matches!(
        perform_verified_clipboard_paste(text, preferred_target),
        PasteAttempt::Failed
    )
}

#[cfg(target_os = "macos")]
pub(crate) fn perform_verified_clipboard_paste(
    text: &str,
    preferred_target: Option<&PasteTargetInfo>,
) -> PasteAttempt {
    let Some(target) = resolve_paste_target_info(preferred_target) else {
        return PasteAttempt::Failed;
    };

    // スナップショット取得は Cmd+V 送信前なので安全に再試行できる。
    // 送信後の再試行は二重貼り付けの危険があるため一切行わない。
    // スナップショットが取れない場合(WKWebView 等、AX がテキストを公開しない
    // ターゲット)でも、編集可能要素として解決済みなので楽観的に貼り付ける。
    let focused = AxFocusedText::capture_for_paste_target(&target).or_else(|| {
        std::thread::sleep(Duration::from_millis(PASTE_SNAPSHOT_RETRY_DELAY_MS));
        AxFocusedText::capture_for_paste_target(&target)
    });

    let original = read_clipboard_text();
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if clipboard.set_text(text.to_string()).is_err() {
            return PasteAttempt::Failed;
        }
    } else {
        return PasteAttempt::Failed;
    }
    std::thread::sleep(Duration::from_millis(PASTE_WRITE_SETTLE_MS));
    if !run_keystroke("v") {
        restore_clipboard(original);
        return PasteAttempt::Failed;
    }

    let attempt = match focused {
        Some(focused) => match wait_for_verified_insertion(&focused, text) {
            Some((after_paste, insertion)) => PasteAttempt::Verified(Box::new(VerifiedPaste {
                target: focused,
                after_paste,
                insertion,
            })),
            // AX で変化を確認できなくても、対象アプリにフォーカスが残っていれば
            // Cmd+V は届いているとみなす。「実際は貼れているのに失敗扱い」で
            // フォールバックを出すより、ここでは楽観に倒す。
            None => unverified_unless_focus_moved(&target),
        },
        None => {
            // 検証手段がないため、遅いアプリでも貼り付けが処理される時間を
            // 確保してからクリップボードを復元する。
            std::thread::sleep(Duration::from_millis(PASTE_VERIFY_TIMEOUT_MS));
            unverified_unless_focus_moved(&target)
        }
    };
    restore_clipboard(original);
    attempt
}

/// 貼り付け後もフォーカスが対象アプリに残っているかで Unverified / Failed を
/// 判定する。AX が読めない場合は楽観的に Unverified とする。
#[cfg(target_os = "macos")]
fn unverified_unless_focus_moved(target: &PasteTargetInfo) -> PasteAttempt {
    let Some(expected_pid) = target.pid else {
        return PasteAttempt::Unverified;
    };
    match current_paste_target_info().and_then(|current| current.pid) {
        Some(pid) if pid != expected_pid => PasteAttempt::Failed,
        _ => PasteAttempt::Unverified,
    }
}

/// 挿入をポーリングで検証する。検証が取れるか PASTE_VERIFY_TIMEOUT_MS を
/// 超えるまでクリップボードは呼び出し側で復元しないこと。
#[cfg(target_os = "macos")]
fn wait_for_verified_insertion(
    focused: &AxFocusedText,
    text: &str,
) -> Option<(AxTextSnapshot, VerifiedPasteInsertion)> {
    let deadline = Instant::now() + Duration::from_millis(PASTE_VERIFY_TIMEOUT_MS);
    loop {
        if let Some(after_paste) = focused.element.read_text_snapshot() {
            if let Some(insertion) = verify_paste_insertion(&focused.snapshot, &after_paste, text) {
                return Some((after_paste, insertion));
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(PASTE_VERIFY_POLL_MS));
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn verify_paste_insertion(
    before: &AxTextSnapshot,
    after: &AxTextSnapshot,
    text: &str,
) -> Option<VerifiedPasteInsertion> {
    if after.pid != before.pid {
        return None;
    }
    if let Some(range) = inserted_range_from_snapshots(before, after) {
        return Some(VerifiedPasteInsertion::Changed(range));
    }
    if unchanged_selection_replacement_matches_text(before, after, text) {
        return Some(VerifiedPasteInsertion::SameTextReplacement);
    }
    None
}

#[cfg(target_os = "macos")]
pub(crate) fn unchanged_selection_replacement_matches_text(
    before: &AxTextSnapshot,
    after: &AxTextSnapshot,
    text: &str,
) -> bool {
    if text.is_empty() || before.value != after.value || before.selected_range.length == 0 {
        return false;
    }
    if before.selected_range == after.selected_range {
        return false;
    }
    utf16_range_text(&before.value, before.selected_range).is_some_and(|selected| selected == text)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn paste_text(_text: &str, _preferred_target: Option<&PasteTargetInfo>) -> bool {
    false
}

#[cfg(target_os = "macos")]
pub(crate) fn run_keystroke(key: &str) -> bool {
    let Some(keycode) = command_keycode(key) else {
        return false;
    };
    post_command_key(keycode)
}

#[cfg(target_os = "macos")]
pub(crate) fn command_keycode(key: &str) -> Option<u16> {
    match key {
        "c" => Some(8),
        "v" => Some(9),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
pub(crate) type CGEventRef = *mut c_void;

#[cfg(target_os = "macos")]
pub(crate) type CGEventSourceRef = *mut c_void;

#[cfg(target_os = "macos")]
pub(crate) const KCG_HID_EVENT_TAP: u32 = 0;

#[cfg(target_os = "macos")]
pub(crate) const KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: u32 = 1;

#[cfg(target_os = "macos")]
pub(crate) const KCG_EVENT_FLAG_MASK_COMMAND: u64 = 0x0010_0000;

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceCreate(state_id: u32) -> CGEventSourceRef;
    fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        virtual_key: u16,
        key_down: bool,
    ) -> CGEventRef;
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventPost(tap: u32, event: CGEventRef);
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub(crate) fn CFRelease(cf: *const c_void);
}

#[cfg(target_os = "macos")]
pub(crate) fn post_command_key(keycode: u16) -> bool {
    unsafe {
        let source = CGEventSourceCreate(KCG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE);
        if source.is_null() {
            return false;
        }

        let key_down = CGEventCreateKeyboardEvent(source, keycode, true);
        let key_up = CGEventCreateKeyboardEvent(source, keycode, false);
        if key_down.is_null() || key_up.is_null() {
            if !key_down.is_null() {
                CFRelease(key_down.cast_const());
            }
            if !key_up.is_null() {
                CFRelease(key_up.cast_const());
            }
            CFRelease(source.cast_const());
            return false;
        }

        CGEventSetFlags(key_down, KCG_EVENT_FLAG_MASK_COMMAND);
        CGEventSetFlags(key_up, KCG_EVENT_FLAG_MASK_COMMAND);
        CGEventPost(KCG_HID_EVENT_TAP, key_down);
        std::thread::sleep(Duration::from_millis(12));
        CGEventPost(KCG_HID_EVENT_TAP, key_up);

        CFRelease(key_down.cast_const());
        CFRelease(key_up.cast_const());
        CFRelease(source.cast_const());
        true
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn resolve_paste_target_info(
    preferred_target: Option<&PasteTargetInfo>,
) -> Option<PasteTargetInfo> {
    let own_pid = std::process::id() as i32;
    // セッション開始時に Enja 自身(メモ等)を狙っていた場合、または狙いが
    // 不明な場合は、自プロセスの編集可能要素も貼り付け先として認める。
    // 開始時に外部アプリを狙っていた場合は従来どおり自プロセスを除外し、
    // 録音中に Enja へフォーカスが移っていても元のアプリへ貼り付け直す。
    let allow_own = preferred_target.is_none_or(|preferred| preferred.pid == Some(own_pid));
    let target = current_paste_target_info();
    let current_missing = target.is_none();
    let current_is_own = target
        .as_ref()
        .is_some_and(|target| target.pid == Some(own_pid));
    if target
        .as_ref()
        .is_some_and(|target| is_verified_paste_candidate(target, own_pid, allow_own))
    {
        return target;
    }

    let fallback = target
        .as_ref()
        .filter(|target| is_fallback_paste_candidate(target, own_pid, allow_own))
        .cloned();

    if let Some(pid) = manual_accessibility_retry_pid(target.as_ref(), own_pid) {
        if ensure_manual_accessibility_for_pid(pid) {
            for _ in 0..MANUAL_ACCESSIBILITY_POLL_ATTEMPTS {
                std::thread::sleep(MANUAL_ACCESSIBILITY_POLL_INTERVAL);
                let target = current_paste_target_info();
                if target
                    .as_ref()
                    .is_some_and(|target| is_verified_paste_candidate(target, own_pid, allow_own))
                {
                    return target;
                }
                if target
                    .as_ref()
                    .is_some_and(|target| is_fallback_paste_candidate(target, own_pid, allow_own))
                {
                    return target;
                }
            }
        }
    }

    fallback.or_else(|| {
        let preferred = preferred_target
            .filter(|target| is_attemptable_paste_target(target, own_pid, allow_own))?;

        if current_is_own && !allow_own {
            let pid = preferred.pid?;
            if !activate_application_pid(pid) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(PASTE_ACTIVATE_SETTLE_MS));
            let target = current_paste_target_info();
            if target
                .as_ref()
                .is_some_and(|target| is_verified_paste_candidate(target, own_pid, allow_own))
            {
                return target;
            }
            if target
                .as_ref()
                .is_some_and(|target| is_fallback_paste_candidate(target, own_pid, allow_own))
            {
                return target;
            }
            return Some(preferred.clone());
        }

        if current_missing {
            Some(preferred.clone())
        } else if allow_own && current_is_own {
            // 自プロセス宛の貼り付けで、現在のフォーカス要素を編集可能と確認
            // できなかった場合も、開始時に狙った要素を信じて試行する。
            Some(preferred.clone())
        } else {
            None
        }
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn current_paste_target_info() -> Option<PasteTargetInfo> {
    current_ax_focused_target_info().or_else(current_system_events_paste_target_info)
}

#[cfg(target_os = "macos")]
pub(crate) fn current_ax_focused_target_info() -> Option<PasteTargetInfo> {
    let focused = AxFocusedElement::capture()?;
    focused.read_paste_target_info()
}

#[cfg(target_os = "macos")]
pub(crate) fn current_system_events_paste_target_info() -> Option<PasteTargetInfo> {
    let script = r#"
tell application "System Events"
  try
    set frontApp to first application process whose frontmost is true
    set pidValue to unix id of frontApp as text
    set roleValue to ""
    set subroleValue to ""
    set attributeNames to {}
    try
      set focusedElement to value of attribute "AXFocusedUIElement" of frontApp
      try
        set roleValue to value of attribute "AXRole" of focusedElement as text
      end try
      try
        set subroleValue to value of attribute "AXSubrole" of focusedElement as text
      end try
      try
        set attributeNames to name of every attribute of focusedElement
      end try
    end try
    set oldDelimiters to AppleScript's text item delimiters
    set AppleScript's text item delimiters to ","
    set attributeNamesText to attributeNames as text
    set AppleScript's text item delimiters to oldDelimiters
    return pidValue & linefeed & roleValue & linefeed & subroleValue & linefeed & attributeNamesText
  on error
    return ""
  end try
end tell
"#;
    std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| PasteTargetInfo::from_osascript_output(&String::from_utf8_lossy(&o.stdout)))
}

#[cfg(target_os = "macos")]
pub(crate) fn activate_application_pid(pid: i32) -> bool {
    let script = format!(
        r#"
tell application "System Events"
  try
    set frontmost of first application process whose unix id is {pid} to true
    return "ok"
  on error
    return ""
  end try
end tell
"#
    );
    std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "ok")
}

pub(crate) fn manual_accessibility_retry_pid(
    target: Option<&PasteTargetInfo>,
    own_pid: i32,
) -> Option<i32> {
    let target = target?;
    if is_pasteable_target(target) {
        return None;
    }
    let pid = target.pid?;
    if pid == own_pid {
        return None;
    }
    Some(pid)
}

#[cfg(target_os = "macos")]
pub(crate) fn ensure_manual_accessibility_for_pid(pid: i32) -> bool {
    if recently_failed_manual_accessibility(pid) {
        return false;
    }
    if manual_accessibility_is_enabled(pid) {
        return true;
    }

    if enable_manual_accessibility_for_pid(pid) {
        remember_manual_accessibility_enabled(pid);
        true
    } else {
        remember_manual_accessibility_failure(pid);
        false
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn enable_manual_accessibility_for_pid(pid: i32) -> bool {
    unsafe {
        let app = AXUIElementCreateApplication(pid as c_int);
        if app.is_null() {
            return false;
        }

        let attribute = CFString::new("AXManualAccessibility");
        let enabled = CFBoolean::true_value();
        let status = AXUIElementSetAttributeValue(
            app,
            attribute.as_concrete_TypeRef(),
            enabled.as_CFTypeRef(),
        );
        CFRelease(app.cast());
        status == KAX_ERROR_SUCCESS
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn manual_accessibility_cache() -> &'static Mutex<ManualAccessibilityCache> {
    MANUAL_ACCESSIBILITY_CACHE.get_or_init(|| Mutex::new(ManualAccessibilityCache::default()))
}

#[cfg(target_os = "macos")]
pub(crate) fn manual_accessibility_is_enabled(pid: i32) -> bool {
    manual_accessibility_cache()
        .lock()
        .is_ok_and(|cache| cache.enabled_pids.contains(&pid))
}

#[cfg(target_os = "macos")]
pub(crate) fn remember_manual_accessibility_enabled(pid: i32) {
    if let Ok(mut cache) = manual_accessibility_cache().lock() {
        cache.enabled_pids.insert(pid);
        cache.failed_until_by_pid.remove(&pid);
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn recently_failed_manual_accessibility(pid: i32) -> bool {
    let Ok(mut cache) = manual_accessibility_cache().lock() else {
        return false;
    };
    let Some(until) = cache.failed_until_by_pid.get(&pid).copied() else {
        return false;
    };
    if Instant::now() < until {
        return true;
    }
    cache.failed_until_by_pid.remove(&pid);
    false
}

#[cfg(target_os = "macos")]
pub(crate) fn remember_manual_accessibility_failure(pid: i32) {
    if let Ok(mut cache) = manual_accessibility_cache().lock() {
        cache
            .failed_until_by_pid
            .insert(pid, Instant::now() + MANUAL_ACCESSIBILITY_FAILURE_TTL);
    }
}

pub(crate) fn is_pasteable_target(target: &PasteTargetInfo) -> bool {
    if is_text_input_role(&target.role) || is_text_input_role(&target.subrole) {
        return true;
    }

    // CGEventPost only tells us the shortcut was emitted, not that any app inserted
    // text. Unknown accessibility roles must fall back unless they expose a cursor.
    target.attributes.contains("AXSelectedTextRange")
        || target.attributes.contains("AXInsertionPointLineNumber")
        || target.attributes.contains("AXSelectedTextMarkerRange")
        || target.attributes.contains("AXEditableAncestor")
}

pub(crate) fn is_verified_paste_candidate(
    target: &PasteTargetInfo,
    own_pid: i32,
    allow_own: bool,
) -> bool {
    (allow_own || target.pid != Some(own_pid)) && is_pasteable_target(target)
}

pub(crate) fn is_attemptable_paste_target(
    target: &PasteTargetInfo,
    own_pid: i32,
    allow_own: bool,
) -> bool {
    if !allow_own && target.pid == Some(own_pid) {
        return false;
    }

    is_pasteable_target(target) || is_fallback_paste_candidate(target, own_pid, allow_own)
}

pub(crate) fn is_fallback_paste_candidate(
    target: &PasteTargetInfo,
    own_pid: i32,
    allow_own: bool,
) -> bool {
    is_web_content_paste_candidate(target, own_pid, allow_own)
        || is_ambiguous_external_paste_candidate(target, own_pid)
}

pub(crate) fn is_web_content_paste_candidate(
    target: &PasteTargetInfo,
    own_pid: i32,
    allow_own: bool,
) -> bool {
    if !allow_own && target.pid == Some(own_pid) {
        return false;
    }

    is_web_content_role(&target.role) || is_web_content_role(&target.subrole)
}

pub(crate) fn is_ambiguous_external_paste_candidate(
    target: &PasteTargetInfo,
    own_pid: i32,
) -> bool {
    target.pid.is_some_and(|pid| pid != own_pid)
        && target.role.is_empty()
        && target.subrole.is_empty()
        && target.attributes.is_empty()
}

pub(crate) fn is_text_input_role(role: &str) -> bool {
    matches!(
        role,
        "AXTextArea" | "AXTextField" | "AXComboBox" | "AXSearchField" | "AXTextView"
    )
}

pub(crate) fn is_web_content_role(role: &str) -> bool {
    role == "AXWebArea"
}

pub(crate) fn read_clipboard_text() -> Option<String> {
    arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get_text().ok())
}

pub(crate) fn write_clipboard_text(value: &str) -> bool {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(value.to_string()))
        .is_ok()
}

pub(crate) fn restore_clipboard(value: Option<String>) {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        match value {
            Some(value) => {
                let _ = clipboard.set_text(value);
            }
            None => {
                let _ = clipboard.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_target_accepts_text_input_roles() {
        assert!(is_pasteable_target(&paste_target("AXTextArea", "", &[])));
        assert!(is_pasteable_target(&paste_target("", "AXTextField", &[])));
    }

    #[test]
    fn paste_target_attempt_skips_own_text_input_unless_allowed() {
        let target = PasteTargetInfo {
            pid: Some(100),
            role: "AXTextArea".to_string(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(is_pasteable_target(&target));
        // 外部アプリを狙ったセッションでは自プロセスを除外する。
        assert!(!is_verified_paste_candidate(&target, 100, false));
        assert!(!is_attemptable_paste_target(&target, 100, false));
        // 開始時に Enja 自身(メモ等)を狙った場合は許可する。
        assert!(is_verified_paste_candidate(&target, 100, true));
        assert!(is_attemptable_paste_target(&target, 100, true));
    }

    #[test]
    fn paste_target_accepts_editor_cursor_attributes() {
        assert!(is_pasteable_target(&paste_target(
            "AXGroup",
            "",
            &["AXRole", "AXSelectedTextRange"]
        )));
        assert!(is_pasteable_target(&paste_target(
            "AXWebArea",
            "",
            &["AXInsertionPointLineNumber"]
        )));
        assert!(is_pasteable_target(&paste_target(
            "AXGroup",
            "",
            &["AXSelectedTextMarkerRange"]
        )));
        assert!(is_pasteable_target(&paste_target(
            "AXStaticText",
            "",
            &["AXEditableAncestor"]
        )));
    }

    #[test]
    fn paste_target_rejects_unclear_roles_without_cursor_attributes() {
        assert!(!is_pasteable_target(&paste_target(
            "AXGroup",
            "",
            &["AXRole", "AXValue"]
        )));
        assert!(!is_pasteable_target(&paste_target(
            "AXWebArea",
            "",
            &["AXRole"]
        )));
        assert!(!is_pasteable_target(&paste_target("AXUnknown", "", &[])));
    }

    #[test]
    fn paste_target_treats_web_area_as_fallback_candidate() {
        assert!(is_web_content_paste_candidate(
            &paste_target("AXWebArea", "", &["AXRole"]),
            100,
            false
        ));
        assert!(is_web_content_paste_candidate(
            &paste_target("", "AXWebArea", &["AXRole"]),
            100,
            false
        ));
    }

    #[test]
    fn paste_target_fallback_skips_own_web_area_unless_allowed() {
        let target = PasteTargetInfo {
            pid: Some(100),
            role: "AXWebArea".to_string(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(!is_web_content_paste_candidate(&target, 100, false));
        assert!(is_web_content_paste_candidate(&target, 100, true));
    }

    #[test]
    fn paste_target_treats_pid_only_external_target_as_fallback_candidate() {
        let target = PasteTargetInfo {
            pid: Some(4242),
            role: String::new(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(is_ambiguous_external_paste_candidate(&target, 100));
        assert!(is_attemptable_paste_target(&target, 100, false));
    }

    #[test]
    fn paste_target_rejects_ambiguous_own_target_even_when_own_allowed() {
        // ロールも属性も無い自プロセス要素(音声オーバーレイ等)には
        // own 許可時でも貼り付けを試みない。
        let target = PasteTargetInfo {
            pid: Some(100),
            role: String::new(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(!is_ambiguous_external_paste_candidate(&target, 100));
        assert!(!is_fallback_paste_candidate(&target, 100, true));
        assert!(!is_attemptable_paste_target(&target, 100, true));
    }

    #[test]
    fn paste_target_rejects_known_non_text_external_target_as_fallback_candidate() {
        let target = PasteTargetInfo {
            pid: Some(4242),
            role: "AXButton".to_string(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(!is_fallback_paste_candidate(&target, 100, false));
        assert!(!is_attemptable_paste_target(&target, 100, false));
    }

    #[test]
    fn paste_target_info_parses_osascript_output() {
        let target = PasteTargetInfo::from_osascript_output(
            "AXGroup\nAXTextArea\nAXRole, AXSelectedTextRange\n",
        )
        .expect("target");

        assert_eq!(target.pid, None);
        assert_eq!(target.role, "AXGroup");
        assert_eq!(target.subrole, "AXTextArea");
        assert!(target.attributes.contains("AXSelectedTextRange"));
    }

    #[test]
    fn paste_target_info_parses_pid_osascript_output() {
        let target = PasteTargetInfo::from_osascript_output(
            "4242\nAXGroup\nAXTextArea\nAXRole, AXSelectedTextRange\n",
        )
        .expect("target");

        assert_eq!(target.pid, Some(4242));
        assert_eq!(target.role, "AXGroup");
        assert_eq!(target.subrole, "AXTextArea");
        assert!(target.attributes.contains("AXSelectedTextRange"));
    }

    #[test]
    fn paste_target_info_keeps_pid_when_focus_is_unavailable() {
        let target = PasteTargetInfo::from_osascript_output("4242\n\n\n\n").expect("target");

        assert_eq!(target.pid, Some(4242));
        assert_eq!(target.role, "");
        assert_eq!(target.subrole, "");
        assert!(target.attributes.is_empty());
    }

    #[test]
    fn paste_target_info_rejects_empty_osascript_output() {
        assert!(PasteTargetInfo::from_osascript_output("").is_none());
        assert!(PasteTargetInfo::from_osascript_output("\n\n").is_none());
    }

    #[test]
    fn manual_accessibility_retry_uses_pid_for_unclear_target() {
        let target = PasteTargetInfo {
            pid: Some(4242),
            role: String::new(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert_eq!(
            manual_accessibility_retry_pid(Some(&target), 100),
            Some(4242)
        );
    }

    #[test]
    fn manual_accessibility_retry_skips_existing_pasteable_target() {
        let target = paste_target("AXTextArea", "", &[]);

        assert_eq!(manual_accessibility_retry_pid(Some(&target), 100), None);
    }

    #[test]
    fn manual_accessibility_retry_skips_own_process() {
        let target = PasteTargetInfo {
            pid: Some(4242),
            role: String::new(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert_eq!(manual_accessibility_retry_pid(Some(&target), 4242), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn verified_paste_accepts_changed_value() {
        let before = ax_text_snapshot("hello ", 6, 0);
        let after = ax_text_snapshot("hello world", 11, 0);

        let Some(VerifiedPasteInsertion::Changed(range)) =
            verify_paste_insertion(&before, &after, "world")
        else {
            panic!("expected changed insertion");
        };

        assert_eq!(range.location, 6);
        assert_eq!(range.length, "world".encode_utf16().count());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn verified_paste_accepts_identical_selected_text_replacement() {
        let selected = "タイプレス";
        let before = ax_text_snapshot(selected, 0, selected.encode_utf16().count());
        let after = ax_text_snapshot(selected, selected.encode_utf16().count(), 0);

        assert!(matches!(
            verify_paste_insertion(&before, &after, selected),
            Some(VerifiedPasteInsertion::SameTextReplacement)
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn verified_paste_rejects_unchanged_cursor_value() {
        let before = ax_text_snapshot("hello", 5, 0);
        let after = ax_text_snapshot("hello", 5, 0);

        assert!(verify_paste_insertion(&before, &after, "world").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn verified_paste_rejects_same_text_when_selection_did_not_move() {
        let selected = "タイプレス";
        let before = ax_text_snapshot(selected, 0, selected.encode_utf16().count());
        let after = ax_text_snapshot(selected, 0, selected.encode_utf16().count());

        assert!(verify_paste_insertion(&before, &after, selected).is_none());
    }

    #[cfg(target_os = "macos")]
    fn ax_text_snapshot(
        value: &str,
        selection_location: usize,
        selection_length: usize,
    ) -> AxTextSnapshot {
        AxTextSnapshot {
            pid: 4242,
            value: value.to_string(),
            selected_range: TextRange {
                location: selection_location,
                length: selection_length,
            },
        }
    }

    fn paste_target(role: &str, subrole: &str, attributes: &[&str]) -> PasteTargetInfo {
        PasteTargetInfo {
            pid: None,
            role: role.to_string(),
            subrole: subrole.to_string(),
            attributes: attributes.iter().map(|value| value.to_string()).collect(),
        }
    }
}
