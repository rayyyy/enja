//! 画面文脈の取得と整形。AX(Accessibility)読み取り・OCRヘルパー起動・
//! ASRヒント語抽出・整形プロンプト用セクション生成を担う。

use super::*;

const SCREEN_CONTEXT_SIDE_CHARS: usize = 1_400;
const SCREEN_CONTEXT_VISIBLE_MAX_CHARS: usize = 6_000;
const SCREEN_CONTEXT_OCR_MAX_CHARS: usize = 6_000;
const SCREEN_CONTEXT_ASR_MAX_TERMS: usize = 180;
const SCREEN_CONTEXT_ASR_MAX_TERM_CHARS: usize = 48;
const SCREEN_CONTEXT_OCR_TIMEOUT: Duration = Duration::from_secs(6);
const SCREEN_CONTEXT_OCR_WAIT_TIMEOUT: Duration = Duration::from_millis(450);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScreenContextHelperResponse {
    ok: bool,
    error: Option<String>,
    app_name: Option<String>,
    window_title: Option<String>,
    text: Option<String>,
    details: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct VoiceScreenContext {
    pub(crate) app_name: Option<String>,
    pub(crate) window_title: Option<String>,
    pub(crate) element_role: Option<String>,
    pub(crate) focused_before: String,
    pub(crate) focused_selection: String,
    pub(crate) focused_after: String,
    pub(crate) visible_text: String,
    pub(crate) ocr_text: String,
    pub(crate) details: Vec<String>,
}

impl VoiceScreenContext {
    pub(crate) fn is_empty(&self) -> bool {
        option_text_is_empty(self.app_name.as_deref())
            && option_text_is_empty(self.window_title.as_deref())
            && self.focused_before.trim().is_empty()
            && self.focused_selection.trim().is_empty()
            && self.focused_after.trim().is_empty()
            && self.visible_text.trim().is_empty()
            && self.ocr_text.trim().is_empty()
    }

    pub(crate) fn merge_ocr(&mut self, ocr: VoiceScreenContextOcr) {
        if option_text_is_empty(self.app_name.as_deref()) {
            self.app_name = ocr.app_name;
        }
        if option_text_is_empty(self.window_title.as_deref()) {
            self.window_title = ocr.window_title;
        }
        self.ocr_text = clamp_chars(&ocr.text, SCREEN_CONTEXT_OCR_MAX_CHARS);
        self.details.extend(ocr.details);
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct VoiceScreenContextOcr {
    pub(crate) app_name: Option<String>,
    pub(crate) window_title: Option<String>,
    pub(crate) text: String,
    pub(crate) details: Vec<String>,
}

pub(crate) struct VoiceScreenContextCapture {
    pub(crate) context: VoiceScreenContext,
    pub(crate) ocr_rx: Option<oneshot::Receiver<Option<VoiceScreenContextOcr>>>,
}

fn option_text_is_empty(value: Option<&str>) -> bool {
    match value {
        Some(value) => value.trim().is_empty(),
        None => true,
    }
}

pub(crate) fn start_voice_screen_context_capture(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    paste_target: Option<&PasteTargetInfo>,
    capture_ocr: bool,
) -> VoiceScreenContextCapture {
    if !settings.voice.screen_context_enabled {
        return VoiceScreenContextCapture {
            context: VoiceScreenContext::default(),
            ocr_rx: None,
        };
    }

    VoiceScreenContextCapture {
        context: capture_voice_screen_context(paste_target),
        ocr_rx: if capture_ocr {
            start_screen_context_ocr_capture(app)
        } else {
            None
        },
    }
}

pub(crate) fn should_capture_voice_screen_context_ocr(
    settings: &AppSettings,
    mode: VoiceMode,
    mode_profile_id: &str,
) -> bool {
    if !settings.voice.screen_context_enabled || !settings.voice.screen_context_ocr_enabled {
        return false;
    }

    match mode {
        VoiceMode::Ask => true,
        VoiceMode::Dictation => {
            let formatting_enabled = settings
                .voice
                .mode_profile_or_default(mode_profile_id)
                .map(|profile| profile.formatting_enabled)
                .unwrap_or(true);
            if formatting_enabled {
                return true;
            }

            !should_use_live_transcript(settings, mode, mode_profile_id)
                && speech_profile_accepts_screen_context_hints(settings.voice.speech_profile)
        }
    }
}

fn speech_profile_accepts_screen_context_hints(profile: SpeechProfile) -> bool {
    match profile {
        SpeechProfile::GoogleChirp3
        | SpeechProfile::OpenAiGpt4oTranscribe
        | SpeechProfile::OpenAiGpt4oMiniTranscribe
        | SpeechProfile::GeminiAudio
        | SpeechProfile::AppleSpeechAnalyzer => true,
    }
}

pub(crate) async fn resolve_voice_screen_context(
    mut context: VoiceScreenContext,
    ocr_rx: Option<oneshot::Receiver<Option<VoiceScreenContextOcr>>>,
) -> VoiceScreenContext {
    let Some(ocr_rx) = ocr_rx else {
        return context;
    };
    if let Ok(Ok(Some(ocr))) = tokio::time::timeout(SCREEN_CONTEXT_OCR_WAIT_TIMEOUT, ocr_rx).await {
        context.merge_ocr(ocr)
    }
    context
}

#[cfg(target_os = "macos")]
fn start_screen_context_ocr_capture(
    app: &tauri::AppHandle,
) -> Option<oneshot::Receiver<Option<VoiceScreenContextOcr>>> {
    let (tx, rx) = oneshot::channel();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = match run_screen_context_ocr_helper(&app) {
            Ok(context) => Some(context),
            Err(err) => {
                eprintln!("[enja] screen OCR context unavailable: {err}");
                None
            }
        };
        let _ = tx.send(result);
    });
    Some(rx)
}

#[cfg(not(target_os = "macos"))]
fn start_screen_context_ocr_capture(
    _app: &tauri::AppHandle,
) -> Option<oneshot::Receiver<Option<VoiceScreenContextOcr>>> {
    None
}

#[cfg(target_os = "macos")]
fn run_screen_context_ocr_helper(app: &tauri::AppHandle) -> Result<VoiceScreenContextOcr, String> {
    let helper = resolve_screen_context_helper(app)?;
    let mut command = std::process::Command::new(&helper);
    command.arg("ocr-screen");
    let output = command_output_with_timeout(
        command,
        SCREEN_CONTEXT_OCR_TIMEOUT,
        &format!("Screen context helper（path: {}）", helper.display()),
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        return Err(if stderr.is_empty() {
            format!("screen context helper failed: {}", output.status)
        } else {
            stderr
        });
    }
    let response: ScreenContextHelperResponse = serde_json::from_str(&stdout)
        .map_err(|err| format!("screen context helper JSON parse failed: {err}: {stdout}"))?;
    if !response.ok {
        return Err(response
            .error
            .unwrap_or_else(|| "screen context helper failed.".to_string()));
    }
    let text = response.text.unwrap_or_default();
    if text.trim().is_empty() {
        return Err("screen OCR text is empty.".to_string());
    }
    Ok(VoiceScreenContextOcr {
        app_name: response.app_name,
        window_title: response.window_title,
        text,
        details: response.details.unwrap_or_default(),
    })
}

#[cfg(target_os = "macos")]
fn resolve_screen_context_helper(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let executable_name = "enja-screen-context-helper";
    let target_name = format!("enja-screen-context-helper-{}", env!("ENJA_TARGET_TRIPLE"));
    let mut candidates = Vec::<PathBuf>::new();
    if let Ok(path) = std::env::var("ENJA_SCREEN_CONTEXT_HELPER_PATH") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join(executable_name));
        }
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join(executable_name));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bin")
            .join(target_name),
    );

    for path in &candidates {
        if path.is_file() {
            return Ok(path.clone());
        }
    }
    Err(format!(
        "screen context helperが見つかりません。探した場所: {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Default)]
struct FrontAppMetadata {
    pid: Option<i32>,
    app_name: Option<String>,
    window_title: Option<String>,
}

#[cfg(target_os = "macos")]
fn capture_voice_screen_context(paste_target: Option<&PasteTargetInfo>) -> VoiceScreenContext {
    let preferred_pid = paste_target.and_then(|target| target.pid);
    let metadata = current_front_app_metadata(preferred_pid).unwrap_or_default();
    let metadata_pid = metadata.pid;
    let mut context = VoiceScreenContext {
        app_name: metadata.app_name,
        window_title: metadata.window_title,
        element_role: paste_target.and_then(paste_target_role_label),
        ..VoiceScreenContext::default()
    };

    if let Some(focused) = AxFocusedText::capture() {
        let matches_target = preferred_pid
            .map(|pid| focused.snapshot.pid == pid)
            .unwrap_or(true);
        if matches_target {
            let (before, selection, after) =
                focused_text_context(&focused.snapshot.value, focused.snapshot.selected_range);
            context.focused_before = before;
            context.focused_selection = selection;
            context.focused_after = after;
        }
    }

    let context_pid = preferred_pid.or(metadata_pid);
    if let Some(pid) = context_pid {
        context.visible_text = collect_accessibility_window_text(pid);
    }

    context
}

#[cfg(not(target_os = "macos"))]
fn capture_voice_screen_context(_paste_target: Option<&PasteTargetInfo>) -> VoiceScreenContext {
    VoiceScreenContext::default()
}

#[cfg(target_os = "macos")]
fn current_front_app_metadata(preferred_pid: Option<i32>) -> Option<FrontAppMetadata> {
    let process_selector = if let Some(pid) = preferred_pid {
        format!("first application process whose unix id is {pid}")
    } else {
        "first application process whose frontmost is true".to_string()
    };
    let script = format!(
        r#"
tell application "System Events"
  try
    set frontApp to {process_selector}
    set pidValue to unix id of frontApp as text
    set appName to name of frontApp as text
    set windowTitle to ""
    try
      set windowTitle to name of front window of frontApp as text
    end try
    return pidValue & linefeed & appName & linefeed & windowTitle
  on error
    return ""
  end try
end tell
"#
    );
    let output = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines().collect::<Vec<_>>();
    if lines.is_empty() || stdout.trim().is_empty() {
        return None;
    }
    let pid = lines
        .first()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .filter(|pid| *pid > 0);
    Some(FrontAppMetadata {
        pid,
        app_name: lines
            .get(1)
            .map(|value| normalize_context_line(value))
            .filter(|value| !value.is_empty()),
        window_title: lines
            .get(2)
            .map(|value| normalize_context_line(value))
            .filter(|value| !value.is_empty()),
    })
}

fn paste_target_role_label(target: &PasteTargetInfo) -> Option<String> {
    let mut parts = Vec::new();
    if !target.role.trim().is_empty() {
        parts.push(target.role.trim());
    }
    if !target.subrole.trim().is_empty() && target.subrole.trim() != target.role.trim() {
        parts.push(target.subrole.trim());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" / "))
    }
}

#[cfg(target_os = "macos")]
fn focused_text_context(value: &str, selected_range: TextRange) -> (String, String, String) {
    let selection_start = utf16_offset_to_byte_index(value, selected_range.location).unwrap_or(0);
    let selection_end =
        utf16_offset_to_byte_index(value, selected_range.end()).unwrap_or(selection_start);
    let before = clamp_chars_from_end(&value[..selection_start], SCREEN_CONTEXT_SIDE_CHARS);
    let selection = if selection_end > selection_start {
        clamp_chars(
            &value[selection_start..selection_end],
            SCREEN_CONTEXT_SIDE_CHARS,
        )
    } else {
        String::new()
    };
    let after = if selection_end < value.len() {
        clamp_chars(&value[selection_end..], SCREEN_CONTEXT_SIDE_CHARS)
    } else {
        String::new()
    };
    (before, selection, after)
}

#[cfg(target_os = "macos")]
fn collect_accessibility_window_text(pid: i32) -> String {
    unsafe {
        let app = AXUIElementCreateApplication(pid as c_int);
        if app.is_null() {
            return String::new();
        }
        let app_element = AxElementRef { raw: app };
        let mut collector = AxVisibleTextCollector::default();
        if let Some(window) =
            copy_ax_attribute_raw(app_element.raw, "AXFocusedWindow").map(|raw| AxElementRef {
                raw: raw as AXUIElementRef,
            })
        {
            collect_ax_visible_text(window.raw, 0, &mut collector);
        } else {
            collect_ax_visible_text(app_element.raw, 0, &mut collector);
        }
        collector.finish()
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct AxVisibleTextCollector {
    lines: Vec<String>,
    seen: HashSet<String>,
    chars: usize,
    nodes: usize,
}

#[cfg(target_os = "macos")]
impl AxVisibleTextCollector {
    fn can_continue(&self) -> bool {
        self.chars < SCREEN_CONTEXT_VISIBLE_MAX_CHARS && self.nodes < 450
    }

    fn push(&mut self, value: &str) {
        if !self.can_continue() {
            return;
        }
        let line = normalize_context_line(value);
        if line.chars().count() < 2 {
            return;
        }
        let key = line.to_lowercase();
        if !self.seen.insert(key) {
            return;
        }
        let remaining = SCREEN_CONTEXT_VISIBLE_MAX_CHARS.saturating_sub(self.chars);
        if remaining == 0 {
            return;
        }
        let clipped = clamp_chars(&line, remaining.min(500));
        self.chars = self.chars.saturating_add(clipped.chars().count() + 1);
        self.lines.push(clipped);
    }

    fn finish(self) -> String {
        self.lines.join("\n")
    }
}

#[cfg(target_os = "macos")]
fn collect_ax_visible_text(
    element: AXUIElementRef,
    depth: usize,
    collector: &mut AxVisibleTextCollector,
) {
    if element.is_null() || depth > 8 || !collector.can_continue() {
        return;
    }
    collector.nodes = collector.nodes.saturating_add(1);

    let role = copy_ax_string_attribute(element, "AXRole").unwrap_or_default();
    let subrole = copy_ax_string_attribute(element, "AXSubrole").unwrap_or_default();
    if is_sensitive_ax_role(&role) || is_sensitive_ax_role(&subrole) {
        return;
    }

    for attribute in ["AXTitle", "AXValue", "AXDescription", "AXHelp"] {
        if let Some(value) = copy_ax_string_attribute(element, attribute) {
            collector.push(&value);
        }
    }

    if depth >= 8 || !collector.can_continue() {
        return;
    }

    let mut visited_children = false;
    for attribute in ["AXVisibleChildren", "AXChildren", "AXRows", "AXColumns"] {
        if for_each_ax_child(element, attribute, |child| {
            visited_children = true;
            collect_ax_visible_text(child, depth + 1, collector);
        }) && visited_children
        {
            break;
        }
    }
}

#[cfg(target_os = "macos")]
fn for_each_ax_child(
    element: AXUIElementRef,
    attribute: &str,
    mut visit: impl FnMut(AXUIElementRef),
) -> bool {
    let Some(value) = copy_ax_attribute_raw(element, attribute) else {
        return false;
    };
    unsafe {
        if CFGetTypeID(value) != CFArrayGetTypeID() {
            CFRelease(value);
            return false;
        }
        let count = CFArrayGetCount(value as CFArrayRef);
        for index in 0..count {
            let child = CFArrayGetValueAtIndex(value as CFArrayRef, index);
            if !child.is_null() {
                visit(child as AXUIElementRef);
            }
        }
        CFRelease(value);
    }
    true
}

fn is_sensitive_ax_role(role: &str) -> bool {
    matches!(role, "AXSecureTextField")
}

pub(crate) fn transcription_prompt_context(
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
) -> String {
    let mut sections = Vec::new();
    let dictionary_context = dictionary::prompt_lines(entries);
    if !dictionary_context.trim().is_empty() {
        sections.push(dictionary_context);
    }
    let terms = screen_context_terms(screen_context);
    if !terms.is_empty() {
        sections.push(format!(
            "画面文脈から抽出したヒント語（聞こえた場合だけ使用）:\n{}",
            terms
                .into_iter()
                .map(|value| format!("- {value}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    sections.join("\n")
}

pub(crate) fn transcription_contextual_phrases(
    entries: &[DictionaryEntry],
    screen_context: &VoiceScreenContext,
    limit: usize,
) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut values = Vec::new();
    for value in dictionary::enabled_phrases(entries)
        .into_iter()
        .chain(screen_context_terms(screen_context))
    {
        if values.len() >= limit {
            break;
        }
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_lowercase();
        if seen.insert(key) {
            values.push(trimmed.to_string());
        }
    }
    values
}

pub(crate) fn screen_context_terms(screen_context: &VoiceScreenContext) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut out = Vec::<String>::new();
    for value in [
        screen_context.app_name.as_deref().unwrap_or_default(),
        screen_context.window_title.as_deref().unwrap_or_default(),
        &screen_context.focused_before,
        &screen_context.focused_selection,
        &screen_context.focused_after,
        &screen_context.visible_text,
        &screen_context.ocr_text,
    ] {
        extract_screen_context_terms(value, &mut seen, &mut out);
        if out.len() >= SCREEN_CONTEXT_ASR_MAX_TERMS {
            break;
        }
    }
    out
}

fn extract_screen_context_terms(text: &str, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    let mut current = String::new();
    for ch in text.chars() {
        if is_context_term_char(ch) {
            current.push(ch);
        } else {
            push_context_term(&current, seen, out);
            current.clear();
        }
        if out.len() >= SCREEN_CONTEXT_ASR_MAX_TERMS {
            return;
        }
    }
    push_context_term(&current, seen, out);
}

fn push_context_term(value: &str, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    if out.len() >= SCREEN_CONTEXT_ASR_MAX_TERMS {
        return;
    }
    let value = value
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '.' | ',' | ':' | ';' | '/' | '\\' | '-' | '_' | '@' | '#' | '$' | '%' | '&' | '*'
            )
        })
        .trim();
    let char_count = value.chars().count();
    if !(2..=SCREEN_CONTEXT_ASR_MAX_TERM_CHARS).contains(&char_count) {
        return;
    }
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        return;
    }
    let key = value.to_lowercase();
    if seen.insert(key) {
        out.push(value.to_string());
    }
}

fn is_context_term_char(ch: char) -> bool {
    ch.is_alphanumeric()
        || matches!(
            ch,
            '_' | '-' | '.' | '/' | '\\' | '@' | '#' | '$' | '%' | '&' | '+' | ':' | '*'
        )
        || ('\u{3040}'..='\u{30ff}').contains(&ch)
        || ('\u{3400}'..='\u{9fff}').contains(&ch)
}

pub(crate) fn finalization_screen_context_section(screen_context: &VoiceScreenContext) -> String {
    if screen_context.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    if let Some(app_name) = screen_context.app_name.as_deref() {
        if !app_name.trim().is_empty() {
            lines.push(format!("アプリ: {}", normalize_context_line(app_name)));
        }
    }
    if let Some(window_title) = screen_context.window_title.as_deref() {
        if !window_title.trim().is_empty() {
            lines.push(format!(
                "ウィンドウ: {}",
                normalize_context_line(window_title)
            ));
        }
    }
    if let Some(role) = screen_context.element_role.as_deref() {
        if !role.trim().is_empty() {
            lines.push(format!("入力先: {}", normalize_context_line(role)));
        }
    }
    push_context_block(
        &mut lines,
        "カーソル前",
        &screen_context.focused_before,
        SCREEN_CONTEXT_SIDE_CHARS,
    );
    push_context_block(
        &mut lines,
        "選択中",
        &screen_context.focused_selection,
        SCREEN_CONTEXT_SIDE_CHARS,
    );
    push_context_block(
        &mut lines,
        "カーソル後",
        &screen_context.focused_after,
        SCREEN_CONTEXT_SIDE_CHARS,
    );
    push_context_block(
        &mut lines,
        "表示テキスト",
        &screen_context.visible_text,
        SCREEN_CONTEXT_VISIBLE_MAX_CHARS,
    );
    push_context_block(
        &mut lines,
        "OCRテキスト",
        &screen_context.ocr_text,
        SCREEN_CONTEXT_OCR_MAX_CHARS,
    );

    if lines.is_empty() {
        String::new()
    } else {
        format!(
            "画面文脈（貼り付け先と周辺表示。音声と矛盾する内容は使わない）:\n{}",
            lines.join("\n")
        )
    }
}

fn push_context_block(lines: &mut Vec<String>, label: &str, value: &str, max_chars: usize) {
    let value = normalize_context_block(value);
    if value.is_empty() {
        return;
    }
    lines.push(format!("{label}:\n{}", clamp_chars(&value, max_chars)));
}

fn normalize_context_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_context_block(value: &str) -> String {
    value
        .lines()
        .map(normalize_context_line)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn clamp_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

fn clamp_chars_from_end(value: &str, max_chars: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return value.to_string();
    }
    chars[chars.len() - max_chars..].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_context_terms_extracts_code_and_product_terms() {
        let context = VoiceScreenContext {
            app_name: Some("Cursor".to_string()),
            window_title: Some("main.ts - enja".to_string()),
            visible_text: "GPT-4o kubectl src-tauri/voice.rs Typeless AquaVoice 12345".to_string(),
            ..VoiceScreenContext::default()
        };

        let terms = screen_context_terms(&context);

        assert!(terms.contains(&"Cursor".to_string()));
        assert!(terms.contains(&"main.ts".to_string()));
        assert!(terms.contains(&"GPT-4o".to_string()));
        assert!(terms.contains(&"kubectl".to_string()));
        assert!(terms.contains(&"src-tauri/voice.rs".to_string()));
        assert!(!terms.contains(&"12345".to_string()));
    }

    #[test]
    fn finalization_screen_context_section_labels_context_sources() {
        let context = VoiceScreenContext {
            app_name: Some("Slack".to_string()),
            window_title: Some("Acme".to_string()),
            focused_before: "了解しました。".to_string(),
            focused_after: "よろしくお願いします。".to_string(),
            ..VoiceScreenContext::default()
        };

        let section = finalization_screen_context_section(&context);

        assert!(section.contains("アプリ: Slack"));
        assert!(section.contains("ウィンドウ: Acme"));
        assert!(section.contains("カーソル前"));
        assert!(section.contains("カーソル後"));
    }

    #[test]
    fn screen_context_ocr_policy_skips_speed_mode_when_live_transcript_is_used() {
        let mut settings = AppSettings::default();
        settings.voice.active_mode_profile_id = "speed".to_string();

        assert!(!should_capture_voice_screen_context_ocr(
            &settings,
            VoiceMode::Dictation,
            "speed"
        ));
    }

    #[test]
    fn screen_context_ocr_policy_keeps_speed_mode_for_batch_asr_hints() {
        let mut settings = AppSettings::default();
        settings.voice.active_mode_profile_id = "speed".to_string();
        settings.voice.speech_profile = SpeechProfile::OpenAiGpt4oTranscribe;

        assert!(should_capture_voice_screen_context_ocr(
            &settings,
            VoiceMode::Dictation,
            "speed"
        ));
    }

    #[test]
    fn screen_context_ocr_policy_keeps_finalized_voice_flows() {
        let settings = AppSettings::default();

        assert!(should_capture_voice_screen_context_ocr(
            &settings,
            VoiceMode::Dictation,
            "default"
        ));
        assert!(should_capture_voice_screen_context_ocr(
            &settings,
            VoiceMode::Ask,
            ""
        ));
    }
}
