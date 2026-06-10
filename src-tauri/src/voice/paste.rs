//! ペースト先の解決・AX 読み取り・クリップボード貼り付けと検証。

#[allow(clippy::wildcard_imports)]
use super::*;

/// 音声オーバーレイのウィンドウタイトル(tauri.conf.json と一致させること)。
#[cfg(target_os = "macos")]
pub(crate) const VOICE_OVERLAY_WINDOW_TITLE: &str = "Enja Voice";

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
    // osascript(System Events)経由は起動・直列処理で数百 ms かかるため、
    // ネイティブ AX 呼び出しで直接 AXSelectedText を読む。
    let focused = AxFocusedElement::capture()?;
    let text = copy_ax_string_attribute(focused.element.raw, "AXSelectedText")?;
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
        // Enja 自身の音声オーバーレイ(tauri.conf.json の "Enja Voice")に
        // フォーカスがある場合は、ロール無しターゲットへ落として貼り付け先
        // 候補から外す。メモ等の編集可能ウィンドウは通常どおり扱う。
        if pid == std::process::id() as i32
            && focused_window_title(self.element.raw).as_deref() == Some(VOICE_OVERLAY_WINDOW_TITLE)
        {
            return Some(PasteTargetInfo {
                pid: Some(pid),
                role: String::new(),
                subrole: String::new(),
                attributes: HashSet::new(),
            });
        }
        Some(PasteTargetInfo {
            pid: Some(pid),
            role: copy_ax_string_attribute(self.element.raw, "AXRole").unwrap_or_default(),
            subrole: copy_ax_string_attribute(self.element.raw, "AXSubrole").unwrap_or_default(),
            attributes: copy_ax_attribute_names(self.element.raw).unwrap_or_default(),
        })
    }
}

/// フォーカス要素が属するウィンドウのタイトルを読む。
#[cfg(target_os = "macos")]
fn focused_window_title(element: AXUIElementRef) -> Option<String> {
    let window = copy_ax_attribute_raw(element, "AXWindow")?;
    let title = copy_ax_string_attribute(window as AXUIElementRef, "AXTitle");
    unsafe {
        CFRelease(window);
    }
    title
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PasteStatus {
    Verified,
    Unverified,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PasteConfirmationKind {
    AxTextChanged,
    AxSameTextReplacement,
    CaretMoved,
    OwnEditablePasteEvent,
    OptimisticTextInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PasteFailureReason {
    NoFocusedTarget,
    FocusedTargetIsNotTextInput,
    NoConfirmationChannel,
    ClipboardUnavailable,
    ClipboardWriteFailed,
    KeystrokeFailed,
    FocusMovedAfterPaste,
    InsertionNotConfirmed,
    #[cfg(not(target_os = "macos"))]
    UnsupportedPlatform,
}

pub(crate) struct PasteReport {
    pub(crate) status: PasteStatus,
    pub(crate) target: Option<PasteTargetInfo>,
    pub(crate) confirmation: Option<PasteConfirmationKind>,
    pub(crate) failure_reason: Option<PasteFailureReason>,
    #[cfg(target_os = "macos")]
    verified: Option<Box<VerifiedPaste>>,
}

impl PasteReport {
    fn new(
        status: PasteStatus,
        target: Option<PasteTargetInfo>,
        confirmation: Option<PasteConfirmationKind>,
        failure_reason: Option<PasteFailureReason>,
    ) -> Self {
        Self {
            status,
            target,
            confirmation,
            failure_reason,
            #[cfg(target_os = "macos")]
            verified: None,
        }
    }

    pub(crate) fn unverified(target: PasteTargetInfo, confirmation: PasteConfirmationKind) -> Self {
        Self::new(
            PasteStatus::Unverified,
            Some(target),
            Some(confirmation),
            None,
        )
    }

    pub(crate) fn failed(
        target: Option<PasteTargetInfo>,
        failure_reason: PasteFailureReason,
    ) -> Self {
        Self::new(PasteStatus::Failed, target, None, Some(failure_reason))
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn verified(
        target: PasteTargetInfo,
        confirmation: PasteConfirmationKind,
        paste: VerifiedPaste,
    ) -> Self {
        let mut report = Self::new(
            PasteStatus::Verified,
            Some(target),
            Some(confirmation),
            None,
        );
        report.verified = Some(Box::new(paste));
        report
    }

    pub(crate) fn inserted(&self) -> bool {
        !matches!(self.status, PasteStatus::Failed)
    }

    pub(crate) fn user_message(&self) -> String {
        match self.failure_reason {
            Some(
                PasteFailureReason::NoFocusedTarget
                | PasteFailureReason::FocusedTargetIsNotTextInput
                | PasteFailureReason::NoConfirmationChannel,
            ) => "入力欄を確認できなかったため、コピー用に表示しています。".to_string(),
            Some(
                PasteFailureReason::ClipboardUnavailable
                | PasteFailureReason::ClipboardWriteFailed
                | PasteFailureReason::KeystrokeFailed,
            ) => "カーソル位置への貼り付けを実行できなかったため、コピー用に表示しています。"
                .to_string(),
            #[cfg(not(target_os = "macos"))]
            Some(PasteFailureReason::UnsupportedPlatform) => {
                "カーソル位置への貼り付けを実行できなかったため、コピー用に表示しています。"
                    .to_string()
            }
            Some(PasteFailureReason::FocusMovedAfterPaste) => {
                "貼り付け中にフォーカスが移動したため、コピー用に表示しています。".to_string()
            }
            Some(PasteFailureReason::InsertionNotConfirmed) | None => {
                "貼り付け後の挿入を確認できなかったため、コピー用に表示しています。".to_string()
            }
        }
    }

    pub(crate) fn debug_summary(&self) -> String {
        format!(
            "status={:?} confirmation={:?} failure={:?} target={}",
            self.status,
            self.confirmation,
            self.failure_reason,
            format_paste_target(self.target.as_ref()),
        )
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn take_verified_paste(&mut self) -> Option<Box<VerifiedPaste>> {
        self.verified.take()
    }
}

pub(crate) fn log_paste_report(report: &PasteReport) {
    if matches!(report.status, PasteStatus::Verified) {
        return;
    }
    eprintln!("[enja] voice paste report: {}", report.debug_summary());
}

fn format_paste_target(target: Option<&PasteTargetInfo>) -> String {
    let Some(target) = target else {
        return "none".to_string();
    };
    let mut attributes = target.attributes.iter().cloned().collect::<Vec<_>>();
    attributes.sort();
    format!(
        "pid={:?} role={:?} subrole={:?} attrs=[{}]",
        target.pid,
        target.role,
        target.subrole,
        attributes.join(","),
    )
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

/// Cmd+V 送信後にポーリングで得られた挿入の証拠。
#[cfg(target_os = "macos")]
enum PasteConfirmation {
    /// AX のテキスト差分で挿入を特定できた。
    Verified {
        after_paste: AxTextSnapshot,
        insertion: VerifiedPasteInsertion,
    },
    /// キャレット移動または自プロセスのペーストイベントで挿入を観測した
    /// (挿入位置までは特定できないので辞書学習には使わない)。
    Observed(PasteConfirmationKind),
}

#[cfg(target_os = "macos")]
pub(crate) fn paste_text(text: &str) -> PasteReport {
    perform_clipboard_paste(text)
}

#[cfg(target_os = "macos")]
pub(crate) fn perform_clipboard_paste(text: &str) -> PasteReport {
    let target = match resolve_paste_target_info() {
        Ok(target) => target,
        Err(failure) => return failure.into_report(),
    };

    let own_pid = std::process::id() as i32;
    let is_own_target = target.pid == Some(own_pid);
    let optimistic = allows_unverified_paste(&target);

    // スナップショット取得は Cmd+V 送信前なので安全に再試行できる。
    // 送信後の再試行は二重貼り付けの危険があるため一切行わない。
    let focused = AxFocusedText::capture_for_paste_target(&target).or_else(|| {
        std::thread::sleep(Duration::from_millis(PASTE_SNAPSHOT_RETRY_DELAY_MS));
        AxFocusedText::capture_for_paste_target(&target)
    });

    // AX がテキスト差分を公開しないターゲットでも、キャレット移動で挿入を
    // 観測できることがある。AXWebArea は非編集ページ本体でもマーカーが動く
    // ことがあるため、キャレット移動だけでは挿入証拠にしない。
    let caret_probe = if allows_caret_movement_confirmation(&target) {
        CaretProbe::capture(&target)
    } else {
        None
    };

    // 挿入を確認する手段がひとつも無く、楽観視も許されないターゲットには
    // Cmd+V を送らない。空打ちした上で成功扱いするより、クリップボードに
    // 触れる前に失敗を確定させてフォールバックダイアログに倒す。
    if focused.is_none() && caret_probe.is_none() && !is_own_target && !optimistic {
        return PasteReport::failed(Some(target), PasteFailureReason::NoConfirmationChannel);
    }

    let original = read_clipboard_text();
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if clipboard.set_text(text.to_string()).is_err() {
            return PasteReport::failed(Some(target), PasteFailureReason::ClipboardWriteFailed);
        }
    } else {
        return PasteReport::failed(Some(target), PasteFailureReason::ClipboardUnavailable);
    }
    std::thread::sleep(Duration::from_millis(PASTE_WRITE_SETTLE_MS));
    let pasted_at = Instant::now();
    if !run_keystroke("v") {
        restore_clipboard(original);
        return PasteReport::failed(Some(target), PasteFailureReason::KeystrokeFailed);
    }

    // 検証が取れるか PASTE_VERIFY_TIMEOUT_MS を超えるまでクリップボードを
    // 復元しない(遅いアプリが復元後に元の内容を貼る競合を防ぐ)。
    let deadline = pasted_at + Duration::from_millis(PASTE_VERIFY_TIMEOUT_MS);
    let confirmed = loop {
        if let Some(focused) = &focused {
            if let Some(after_paste) = focused.element.read_text_snapshot() {
                if let Some(insertion) =
                    verify_paste_insertion(&focused.snapshot, &after_paste, text)
                {
                    break Some(PasteConfirmation::Verified {
                        after_paste,
                        insertion,
                    });
                }
            }
        }
        if is_own_target && own_editable_paste_since(pasted_at) {
            break Some(PasteConfirmation::Observed(
                PasteConfirmationKind::OwnEditablePasteEvent,
            ));
        }
        if caret_probe.as_ref().is_some_and(CaretProbe::caret_moved) {
            break Some(PasteConfirmation::Observed(
                PasteConfirmationKind::CaretMoved,
            ));
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(PASTE_VERIFY_POLL_MS));
    };

    let report = match confirmed {
        Some(PasteConfirmation::Verified {
            after_paste,
            insertion,
        }) => {
            let confirmation = confirmation_kind_for_insertion(&insertion);
            PasteReport::verified(
                target.clone(),
                confirmation,
                VerifiedPaste {
                    target: focused.expect("verified paste requires a snapshot"),
                    after_paste,
                    insertion,
                },
            )
        }
        Some(PasteConfirmation::Observed(confirmation)) => {
            PasteReport::unverified(target.clone(), confirmation)
        }
        // 明示的なテキスト入力ロール等(allows_unverified_paste)だけは、
        // AX に変化が出なくても対象アプリにフォーカスが残っていれば Cmd+V は
        // 届いているとみなす(Electron/Monaco は AX に変化が出ないことがある)。
        // それ以外は確認できなければ失敗としてフォールバックダイアログを出す。
        None if optimistic => unverified_unless_focus_moved(&target),
        None => PasteReport::failed(
            Some(target.clone()),
            PasteFailureReason::InsertionNotConfirmed,
        ),
    };
    restore_clipboard(original);
    report
}

/// 貼り付け後もフォーカスが対象アプリに残っているかで Unverified / Failed を
/// 判定する。AX が読めない場合は楽観的に Unverified とする。
#[cfg(target_os = "macos")]
fn unverified_unless_focus_moved(target: &PasteTargetInfo) -> PasteReport {
    let Some(expected_pid) = target.pid else {
        return PasteReport::unverified(target.clone(), PasteConfirmationKind::OptimisticTextInput);
    };
    match current_paste_target_info().and_then(|current| current.pid) {
        Some(pid) if pid != expected_pid => PasteReport::failed(
            Some(target.clone()),
            PasteFailureReason::FocusMovedAfterPaste,
        ),
        _ => PasteReport::unverified(target.clone(), PasteConfirmationKind::OptimisticTextInput),
    }
}

#[cfg(target_os = "macos")]
fn confirmation_kind_for_insertion(insertion: &VerifiedPasteInsertion) -> PasteConfirmationKind {
    match insertion {
        VerifiedPasteInsertion::Changed(_) => PasteConfirmationKind::AxTextChanged,
        VerifiedPasteInsertion::SameTextReplacement => PasteConfirmationKind::AxSameTextReplacement,
    }
}

/// AX のテキスト差分やキャレット移動で挿入を確認できなかったときに
/// 「貼り付け成功」と楽観視してよいターゲットか。
///
/// ロールや属性名だけでは入力欄かどうかを断定しない。Cursor/Monaco/Electron
/// の隠し textarea や Chrome のページ本体は、見た目には入力欄でなくても
/// AXTextArea / AXSelectedTextRange を出し続けることがある。未確認成功は
/// 「入力できていないのにダイアログが出ない」原因になるため、現状は許可しない。
pub(crate) fn allows_unverified_paste(target: &PasteTargetInfo) -> bool {
    let _ = target;
    false
}

/// キャレット移動を挿入証拠として使ってよいターゲットか。
///
/// AXWebArea の marker range は通常ページ本体でも変化することがあり、Chrome
/// などで偽成功になりやすい。Web コンテンツは AX テキスト差分や paste イベント
/// など、より強い証拠が取れた場合だけ成功扱いにする。
pub(crate) fn allows_caret_movement_confirmation(target: &PasteTargetInfo) -> bool {
    !is_web_content_paste_candidate(target)
}

/// 自プロセスの WebView(メモ・設定画面)で編集可能要素への paste イベントを
/// 最後に観測した時刻。record_editable_paste コマンド経由でフロントエンドが
/// 記録し、自プロセス宛て貼り付けの検証チャネルとして使う。
static LAST_OWN_EDITABLE_PASTE: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

fn own_editable_paste_cell() -> &'static Mutex<Option<Instant>> {
    LAST_OWN_EDITABLE_PASTE.get_or_init(|| Mutex::new(None))
}

pub(crate) fn record_own_editable_paste() {
    if let Ok(mut last) = own_editable_paste_cell().lock() {
        *last = Some(Instant::now());
    }
}

pub(crate) fn own_editable_paste_since(start: Instant) -> bool {
    own_editable_paste_cell()
        .lock()
        .ok()
        .and_then(|last| *last)
        .is_some_and(|at| at >= start)
}

/// スナップショット(AXValue + AXSelectedTextRange)が取れないターゲット向けの
/// 「キャレットが動いた」検出器。挿入が起きればキャレットは必ず進むので、
/// 選択範囲の変化を挿入の証拠として使う(動かない限り何の判断にも使わない、
/// 偽成功を生まない正方向専用のチャネル)。
#[cfg(target_os = "macos")]
struct CaretProbe {
    element: AxElementRef,
    /// AXSelectedTextMarkerRange(WebKit の web area 等)。CFEqual が値比較を
    /// 実装しているかを capture 時に 2 回読みで較正済み。
    marker_range: Option<CFTypeRef>,
    selected_range: Option<TextRange>,
}

#[cfg(target_os = "macos")]
impl CaretProbe {
    fn capture(target: &PasteTargetInfo) -> Option<Self> {
        let focused = AxFocusedElement::capture()?;
        if target
            .pid
            .is_some_and(|pid| focused.element.pid() != Some(pid))
        {
            return None;
        }
        let element = focused.element;
        let selected_range = copy_ax_range_attribute(element.raw, "AXSelectedTextRange");
        let marker_range = calibrated_marker_range(element.raw);
        if selected_range.is_none() && marker_range.is_none() {
            return None;
        }
        Some(Self {
            element,
            marker_range,
            selected_range,
        })
    }

    fn caret_moved(&self) -> bool {
        if let Some(before) = self.selected_range {
            if let Some(now) = copy_ax_range_attribute(self.element.raw, "AXSelectedTextRange") {
                if now != before {
                    return true;
                }
            }
        }
        if let Some(before) = self.marker_range {
            if let Some(now) = copy_ax_attribute_raw(self.element.raw, "AXSelectedTextMarkerRange")
            {
                let moved = unsafe { core_foundation_sys::base::CFEqual(before, now) } == 0;
                unsafe { CFRelease(now) };
                if moved {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(target_os = "macos")]
impl Drop for CaretProbe {
    fn drop(&mut self) {
        if let Some(marker) = self.marker_range.take() {
            unsafe { CFRelease(marker) };
        }
    }
}

/// AXSelectedTextMarkerRange を 2 回読んで CFEqual が真になることを確認する。
/// マーカーが値比較を実装せずポインタ比較に落ちる実装では、毎回「変化した」
/// と誤判定して偽成功(ダイアログを出すべき場面で出さない)になるため、
/// 較正に失敗したらこのチャネル自体を捨てる。
#[cfg(target_os = "macos")]
fn calibrated_marker_range(element: AXUIElementRef) -> Option<CFTypeRef> {
    let first = copy_ax_attribute_raw(element, "AXSelectedTextMarkerRange")?;
    let Some(second) = copy_ax_attribute_raw(element, "AXSelectedTextMarkerRange") else {
        unsafe { CFRelease(first) };
        return None;
    };
    let stable = unsafe { core_foundation_sys::base::CFEqual(first, second) } != 0;
    unsafe { CFRelease(second) };
    if stable {
        Some(first)
    } else {
        unsafe { CFRelease(first) };
        None
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
pub(crate) fn paste_text(_text: &str) -> PasteReport {
    PasteReport::failed(None, PasteFailureReason::UnsupportedPlatform)
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
pub(crate) struct PasteTargetResolutionFailure {
    target: Option<PasteTargetInfo>,
    reason: PasteFailureReason,
}

#[cfg(target_os = "macos")]
impl PasteTargetResolutionFailure {
    fn into_report(self) -> PasteReport {
        PasteReport::failed(self.target, self.reason)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn resolve_paste_target_info() -> Result<PasteTargetInfo, PasteTargetResolutionFailure> {
    // ルールは一つ: 確定時にカーソル(フォーカス)がある編集可能要素に貼る。
    // 解決できなければ理由付き失敗を返し、呼び出し側がコピー用ダイアログを出す。
    let own_pid = std::process::id() as i32;
    let target = current_paste_target_info();
    if let Some(target) = target
        .as_ref()
        .filter(|target| is_verified_paste_candidate(target))
    {
        return Ok(target.clone());
    }

    let fallback = target
        .as_ref()
        .filter(|target| is_fallback_paste_candidate(target, own_pid))
        .cloned();

    if let Some(pid) = manual_accessibility_retry_pid(target.as_ref(), own_pid) {
        if ensure_manual_accessibility_for_pid(pid) {
            for _ in 0..MANUAL_ACCESSIBILITY_POLL_ATTEMPTS {
                std::thread::sleep(MANUAL_ACCESSIBILITY_POLL_INTERVAL);
                let target = current_paste_target_info();
                if let Some(target) = target
                    .as_ref()
                    .filter(|target| is_verified_paste_candidate(target))
                {
                    return Ok(target.clone());
                }
                if let Some(target) = target
                    .as_ref()
                    .filter(|target| is_fallback_paste_candidate(target, own_pid))
                {
                    return Ok(target.clone());
                }
            }
        }
    }

    if let Some(fallback) = fallback {
        return Ok(fallback);
    }

    Err(PasteTargetResolutionFailure {
        reason: if target.is_some() {
            PasteFailureReason::FocusedTargetIsNotTextInput
        } else {
            PasteFailureReason::NoFocusedTarget
        },
        target,
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

// 貼り付け先は「確定時にカーソル(フォーカス)がある編集可能要素」を常に優先する。
// Enja 自身のメモ等も他アプリと同格に扱う(音声オーバーレイは
// read_paste_target_info でロール無しに落ちるため、ここには到達しない)。
pub(crate) fn is_verified_paste_candidate(target: &PasteTargetInfo) -> bool {
    is_pasteable_target(target)
}

pub(crate) fn is_fallback_paste_candidate(target: &PasteTargetInfo, own_pid: i32) -> bool {
    is_web_content_paste_candidate(target) || is_ambiguous_external_paste_candidate(target, own_pid)
}

pub(crate) fn is_web_content_paste_candidate(target: &PasteTargetInfo) -> bool {
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
    fn paste_target_accepts_own_text_input() {
        // Enja 自身のメモ等、自プロセスの編集可能要素も貼り付け先になる。
        let target = PasteTargetInfo {
            pid: Some(100),
            role: "AXTextArea".to_string(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(is_pasteable_target(&target));
        assert!(is_verified_paste_candidate(&target));
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
        assert!(is_web_content_paste_candidate(&paste_target(
            "AXWebArea",
            "",
            &["AXRole"]
        )));
        assert!(is_web_content_paste_candidate(&paste_target(
            "",
            "AXWebArea",
            &["AXRole"]
        )));
    }

    #[test]
    fn paste_target_accepts_own_web_area() {
        // Enja のメモ(WKWebView)もフォールバック候補として扱う。
        let target = PasteTargetInfo {
            pid: Some(100),
            role: "AXWebArea".to_string(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(is_web_content_paste_candidate(&target));
        assert!(is_fallback_paste_candidate(&target, 100));
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
        assert!(is_fallback_paste_candidate(&target, 100));
    }

    #[test]
    fn paste_target_rejects_roleless_own_target() {
        // ロールも属性も無い自プロセス要素(音声オーバーレイは
        // read_paste_target_info でこの形に落ちる)には貼り付けを試みない。
        let target = PasteTargetInfo {
            pid: Some(100),
            role: String::new(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(!is_ambiguous_external_paste_candidate(&target, 100));
        assert!(!is_fallback_paste_candidate(&target, 100));
    }

    #[test]
    fn paste_target_rejects_known_non_text_external_target_as_fallback_candidate() {
        let target = PasteTargetInfo {
            pid: Some(4242),
            role: "AXButton".to_string(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(!is_fallback_paste_candidate(&target, 100));
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

    #[test]
    fn paste_report_failed_user_messages_are_grouped() {
        let no_input = PasteReport::failed(None, PasteFailureReason::NoFocusedTarget);
        assert_eq!(
            no_input.user_message(),
            "入力欄を確認できなかったため、コピー用に表示しています。"
        );

        let not_confirmed = PasteReport::failed(None, PasteFailureReason::InsertionNotConfirmed);
        assert_eq!(
            not_confirmed.user_message(),
            "貼り付け後の挿入を確認できなかったため、コピー用に表示しています。"
        );
    }

    #[test]
    fn paste_report_debug_summary_includes_sorted_target_attributes() {
        let report = PasteReport::failed(
            Some(paste_target("AXGroup", "AXTextArea", &["BAttr", "AAttr"])),
            PasteFailureReason::FocusedTargetIsNotTextInput,
        );

        assert!(report.debug_summary().contains("status=Failed"));
        assert!(report
            .debug_summary()
            .contains("role=\"AXGroup\" subrole=\"AXTextArea\" attrs=[AAttr,BAttr]"));
    }

    #[test]
    fn paste_report_unverified_counts_as_inserted() {
        let report = PasteReport::unverified(
            paste_target("AXTextArea", "", &[]),
            PasteConfirmationKind::OptimisticTextInput,
        );

        assert!(report.inserted());
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

    #[test]
    fn unverified_paste_rejected_for_explicit_text_input_roles() {
        // Cursor/Monaco/Electron の隠し textarea は、見た目には入力欄でない
        // 場所でも AXTextArea として残ることがある。ロールだけでは未確認成功
        // にせず、AX 差分・キャレット移動などの証拠を要求する。
        assert!(!allows_unverified_paste(&paste_target(
            "AXTextArea",
            "",
            &[]
        )));
        assert!(!allows_unverified_paste(&paste_target(
            "",
            "AXTextField",
            &[]
        )));
    }

    #[test]
    fn unverified_paste_rejected_for_non_web_cursor_attributes() {
        assert!(!allows_unverified_paste(&paste_target(
            "AXGroup",
            "",
            &["AXRole", "AXSelectedTextRange"]
        )));
    }

    #[test]
    fn caret_movement_confirmation_rejected_for_web_areas() {
        assert!(!allows_caret_movement_confirmation(&paste_target(
            "AXWebArea",
            "",
            &["AXSelectedTextMarkerRange", "AXInsertionPointLineNumber"]
        )));
        assert!(!allows_caret_movement_confirmation(&paste_target(
            "",
            "AXWebArea",
            &["AXSelectedTextRange"]
        )));
    }

    #[test]
    fn caret_movement_confirmation_allowed_for_non_web_targets() {
        assert!(allows_caret_movement_confirmation(&paste_target(
            "AXTextArea",
            "",
            &["AXSelectedTextRange"]
        )));
        assert!(allows_caret_movement_confirmation(&paste_target(
            "AXGroup",
            "",
            &["AXSelectedTextRange"]
        )));
    }

    #[test]
    fn unverified_paste_rejected_for_web_areas() {
        // web area の属性名はキャレットの有無と無関係に並ぶ(Safari のページ
        // 本体も AXSelectedTextMarkerRange を名乗る)ため、楽観視すると
        // 「カーソルがどこにも無いのに成功扱い」になりダイアログが出なくなる。
        assert!(!allows_unverified_paste(&paste_target(
            "AXWebArea",
            "",
            &["AXRole"]
        )));
        assert!(!allows_unverified_paste(&paste_target(
            "AXWebArea",
            "",
            &["AXSelectedTextMarkerRange", "AXInsertionPointLineNumber"]
        )));
        assert!(!allows_unverified_paste(&paste_target(
            "",
            "AXWebArea",
            &["AXSelectedTextRange"]
        )));
    }

    #[test]
    fn unverified_paste_rejected_for_ambiguous_targets() {
        // pid しか分からないターゲットは編集可能の証拠が無い。確認できなければ
        // フォールバックダイアログに倒す。
        let target = PasteTargetInfo {
            pid: Some(4242),
            role: String::new(),
            subrole: String::new(),
            attributes: HashSet::new(),
        };

        assert!(!allows_unverified_paste(&target));
        assert!(!allows_unverified_paste(&paste_target(
            "AXUnknown",
            "",
            &[]
        )));
    }

    #[test]
    fn own_editable_paste_channel_reports_only_events_after_start() {
        let before_record = Instant::now();
        record_own_editable_paste();
        let after_record = Instant::now();

        assert!(own_editable_paste_since(before_record));
        assert!(!own_editable_paste_since(
            after_record + Duration::from_millis(1)
        ));
    }
}
