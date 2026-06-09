use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, Runtime};

const DEFAULT_NOTE_WIDTH: f64 = 420.0;
const DEFAULT_NOTE_HEIGHT: f64 = 520.0;
const MIN_NOTE_WIDTH: f64 = 300.0;
const MIN_NOTE_HEIGHT: f64 = 260.0;
const NOTE_COLORS: [&str; 5] = ["lemon", "mint", "sky", "rose", "paper"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StickyNote {
    pub id: String,
    pub title: String,
    pub content: Value,
    pub color: String,
    pub pinned: bool,
    pub window: StickyNoteWindowState,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StickyNoteWindowState {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StickyNoteInput {
    pub id: String,
    pub title: String,
    pub content: Value,
    pub color: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredNoteImage {
    pub path: String,
    pub file_name: String,
    pub mime_type: String,
}

impl Default for StickyNoteWindowState {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            width: DEFAULT_NOTE_WIDTH,
            height: DEFAULT_NOTE_HEIGHT,
        }
    }
}

pub fn load_notes<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<StickyNote>, String> {
    let path = notes_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let notes: Vec<StickyNote> = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    Ok(notes.into_iter().map(normalize_note).collect())
}

pub fn create_note<R: Runtime>(app: &AppHandle<R>) -> Result<StickyNote, String> {
    let mut notes = load_notes(app)?;
    let now = now_millis();
    let note = StickyNote {
        id: format!("note-{now}"),
        title: "新しいメモ".to_string(),
        content: default_content(),
        color: "lemon".to_string(),
        pinned: false,
        window: StickyNoteWindowState::default(),
        created_at: now,
        updated_at: now,
    };
    notes.insert(0, note.clone());
    save_notes(app, &notes)?;
    emit_notes_changed(app, &notes);
    Ok(note)
}

pub fn update_note<R: Runtime>(
    app: &AppHandle<R>,
    input: StickyNoteInput,
) -> Result<StickyNote, String> {
    let mut notes = load_notes(app)?;
    let Some(index) = notes.iter().position(|note| note.id == input.id) else {
        return Err("メモが見つかりません。".to_string());
    };

    let mut note = notes[index].clone();
    note.title = normalize_title(&input.title);
    note.content = normalize_content(input.content);
    note.color = normalize_color(&input.color);
    note.updated_at = now_millis();
    notes[index] = note.clone();
    sort_notes(&mut notes);
    save_notes(app, &notes)?;
    if let Some(window) = app.get_webview_window(&sticky_window_label(&note.id)) {
        let _ = window.set_title(&note.title);
    }
    emit_notes_changed(app, &notes);
    Ok(note)
}

pub fn delete_note<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<(), String> {
    let mut notes = load_notes(app)?;
    let before = notes.len();
    notes.retain(|note| note.id != id);
    if notes.len() == before {
        return Err("メモが見つかりません。".to_string());
    }

    if let Some(window) = app.get_webview_window(&sticky_window_label(id)) {
        let _ = window.close();
    }
    save_notes(app, &notes)?;
    emit_notes_changed(app, &notes);
    Ok(())
}

pub fn show_sticky_window<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<(), String> {
    let mut notes = load_notes(app)?;
    let Some(index) = notes.iter().position(|note| note.id == id) else {
        return Err("メモが見つかりません。".to_string());
    };
    notes[index].pinned = true;
    notes[index].updated_at = now_millis();
    let note = notes[index].clone();
    save_notes(app, &notes)?;
    open_sticky_window(app, &note)?;
    emit_notes_changed(app, &notes);
    Ok(())
}

pub fn hide_sticky_window<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<(), String> {
    set_note_pinned(app, id, false)?;
    if let Some(window) = app.get_webview_window(&sticky_window_label(id)) {
        let _ = window.close();
    }
    Ok(())
}

pub fn restore_pinned_windows<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    for note in load_notes(app)?.into_iter().filter(|note| note.pinned) {
        if let Err(err) = open_sticky_window(app, &note) {
            eprintln!("[enja] sticky note restore failed: {err}");
        }
    }
    Ok(())
}

pub fn save_image<R: Runtime>(
    app: &AppHandle<R>,
    note_id: &str,
    mime_type: &str,
    data_base64: &str,
    file_name: Option<String>,
) -> Result<StoredNoteImage, String> {
    let extension =
        extension_for_mime(mime_type).ok_or_else(|| "対応していない画像形式です。".to_string())?;
    let payload = data_base64
        .rsplit_once(',')
        .map(|(_, encoded)| encoded)
        .unwrap_or(data_base64);
    let bytes = general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| e.to_string())?;
    let dir = images_dir(app)?.join(sanitize_path_part(note_id));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let now = now_millis();
    let base_file_name = file_name
        .and_then(|name| sanitize_file_name(&name))
        .unwrap_or_else(|| format!("image-{now}.{extension}"));
    let file_name = ensure_extension(format!("{now}-{base_file_name}"), extension);
    let path = dir.join(&file_name);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;

    Ok(StoredNoteImage {
        path: path.to_string_lossy().to_string(),
        file_name,
        mime_type: mime_type.to_string(),
    })
}

pub fn record_window_geometry<R: Runtime>(window: &tauri::Window<R>) {
    let Some(id) = sticky_note_id_from_label(window.label()) else {
        return;
    };
    let app = window.app_handle();
    let scale = window.scale_factor().unwrap_or(1.0).max(0.1);
    let position = window.outer_position().ok();
    let size = window.outer_size().ok();
    let Some(size) = size else {
        return;
    };
    let state = StickyNoteWindowState {
        x: position.map(|position| f64::from(position.x) / scale),
        y: position.map(|position| f64::from(position.y) / scale),
        width: (f64::from(size.width) / scale).max(MIN_NOTE_WIDTH),
        height: (f64::from(size.height) / scale).max(MIN_NOTE_HEIGHT),
    };
    if let Err(err) = update_window_state(app, &id, state) {
        eprintln!("[enja] sticky note geometry save failed: {err}");
    }
}

pub fn handle_sticky_close<R: Runtime>(window: &tauri::Window<R>) {
    let Some(id) = sticky_note_id_from_label(window.label()) else {
        return;
    };
    if let Err(err) = set_note_pinned(window.app_handle(), &id, false) {
        eprintln!("[enja] sticky note close save failed: {err}");
    }
}

fn open_sticky_window<R: Runtime>(app: &AppHandle<R>, note: &StickyNote) -> Result<(), String> {
    let label = sticky_window_label(&note.id);
    if let Some(window) = app.get_webview_window(&label) {
        window.show().map_err(|e| e.to_string())?;
        let _ = window.set_always_on_top(true);
        let _ = window.set_focus();
        return Ok(());
    }

    let mut builder =
        tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::App("index.html".into()))
            .title(&note.title)
            .inner_size(note.window.width, note.window.height)
            .min_inner_size(MIN_NOTE_WIDTH, MIN_NOTE_HEIGHT)
            .resizable(true)
            .maximizable(false)
            .decorations(true)
            .hidden_title(true)
            .title_bar_style(tauri::TitleBarStyle::Transparent)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(true)
            .visible(true)
            .disable_drag_drop_handler()
            .prevent_overflow();

    if let (Some(x), Some(y)) = (note.window.x, note.window.y) {
        builder = builder.position(x, y);
    } else {
        builder = builder.center();
    }

    builder.build().map_err(|e| e.to_string())?;
    Ok(())
}

fn set_note_pinned<R: Runtime>(app: &AppHandle<R>, id: &str, pinned: bool) -> Result<(), String> {
    let mut notes = load_notes(app)?;
    let Some(index) = notes.iter().position(|note| note.id == id) else {
        return Err("メモが見つかりません。".to_string());
    };
    notes[index].pinned = pinned;
    notes[index].updated_at = now_millis();
    save_notes(app, &notes)?;
    emit_notes_changed(app, &notes);
    Ok(())
}

fn update_window_state<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    state: StickyNoteWindowState,
) -> Result<(), String> {
    let mut notes = load_notes(app)?;
    let Some(index) = notes.iter().position(|note| note.id == id) else {
        return Ok(());
    };
    notes[index].window = state;
    save_notes(app, &notes)
}

fn save_notes<R: Runtime>(app: &AppHandle<R>, notes: &[StickyNote]) -> Result<(), String> {
    let path = notes_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(notes).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn emit_notes_changed<R: Runtime>(app: &AppHandle<R>, notes: &[StickyNote]) {
    let _ = app.emit("sticky-notes-changed", notes);
}

fn notes_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("notes").join("notes.json"))
}

fn images_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("notes").join("images"))
}

fn normalize_note(mut note: StickyNote) -> StickyNote {
    note.title = normalize_title(&note.title);
    note.content = normalize_content(note.content);
    note.color = normalize_color(&note.color);
    note.window.width = note.window.width.max(MIN_NOTE_WIDTH);
    note.window.height = note.window.height.max(MIN_NOTE_HEIGHT);
    note
}

fn normalize_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "無題のメモ".to_string()
    } else {
        title.chars().take(120).collect()
    }
}

fn normalize_content(content: Value) -> Value {
    if content.get("type").and_then(Value::as_str) == Some("doc") {
        content
    } else {
        default_content()
    }
}

fn normalize_color(color: &str) -> String {
    if NOTE_COLORS.contains(&color) {
        color.to_string()
    } else {
        "lemon".to_string()
    }
}

fn default_content() -> Value {
    json!({
        "type": "doc",
        "content": [
            { "type": "paragraph" }
        ]
    })
}

fn sort_notes(notes: &mut [StickyNote]) {
    notes.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
}

fn sticky_window_label(id: &str) -> String {
    format!("sticky-{id}")
}

fn sticky_note_id_from_label(label: &str) -> Option<String> {
    label.strip_prefix("sticky-").map(ToString::to_string)
}

fn extension_for_mime(mime_type: &str) -> Option<&'static str> {
    match mime_type {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

fn sanitize_path_part(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect::<String>()
}

fn sanitize_file_name(value: &str) -> Option<String> {
    let sanitized = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    let sanitized = sanitized.trim_matches('-').trim_matches('.').to_string();
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized.chars().take(80).collect())
    }
}

fn ensure_extension(file_name: String, extension: &str) -> String {
    let suffix = format!(".{extension}");
    if file_name.to_ascii_lowercase().ends_with(&suffix) {
        file_name
    } else {
        format!("{file_name}.{extension}")
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
