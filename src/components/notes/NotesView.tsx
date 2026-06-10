import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Check,
  Pin,
  PinOff,
  Plus,
  Search,
  StickyNote as StickyNoteIcon,
  Trash2,
} from "lucide-react";
import {
  createStickyNote,
  deleteStickyNote,
  hideStickyNoteWindow,
  getStickyNotes,
  showStickyNoteWindow,
  updateStickyNote,
} from "../../lib/commands";
import {
  deriveNoteTitle,
  extractPlainText,
  normalizeRichTextNode,
  noteColorClass,
  noteColorPresets,
  serializeRichTextNode,
} from "../../lib/stickyNotes";
import type { StickyNote } from "../../types";
import { WindowDragRegion } from "../ui";
import { RichNoteEditor } from "./RichNoteEditor";

type NoteContextMenuState = {
  noteId: string;
  x: number;
  y: number;
} | null;

type NoteListRow = {
  note: StickyNote;
  title: string;
  preview: string;
  searchText: string;
};

const NOTE_CONTEXT_MENU_MARGIN = 8;
const NOTE_CONTEXT_MENU_WIDTH = 132;
const NOTE_CONTEXT_MENU_HEIGHT = 42;

export function NotesView() {
  const {
    notes,
    loaded,
    patchNote,
    createNote,
    removeNote,
    showPinned,
    hidePinned,
  } = useStickyNotes({ createWhenEmpty: true });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [focusedNoteId, setFocusedNoteId] = useState<string | null>(null);
  const [editorFocusNoteId, setEditorFocusNoteId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [contextMenu, setContextMenu] = useState<NoteContextMenuState>(null);

  const selectNote = useCallback((id: string | null) => {
    setSelectedId(id);
    setFocusedNoteId(id);
  }, []);

  const createAndSelectNote = useCallback(() => {
    setContextMenu(null);
    void createNote()
      .then((note) => {
        setEditorFocusNoteId(note.id);
        selectNote(note.id);
      })
      .catch((error) => {
        console.error("[enja] sticky note create failed", error);
      });
  }, [createNote, selectNote]);

  const deleteNote = useCallback(
    (id: string) => {
      setContextMenu(null);
      const nextSelectedId =
        selectedId === id
          ? notes.find((note) => note.id !== id)?.id ?? null
          : selectedId;
      setSelectedId(nextSelectedId);
      setFocusedNoteId(nextSelectedId);
      void removeNote(id);
    },
    [notes, removeNote, selectedId],
  );

  useEffect(() => {
    if (!loaded) return;
    if (notes.length === 0) {
      selectNote(null);
      return;
    }
    if (!selectedId || !notes.some((note) => note.id === selectedId)) {
      selectNote(notes[0].id);
    }
  }, [loaded, notes, selectedId]);

  useEffect(() => {
    if (!focusedNoteId) return;
    if (notes.some((note) => note.id === focusedNoteId)) return;
    setFocusedNoteId(selectedId);
  }, [focusedNoteId, notes, selectedId]);

  // タイトル/プレビューの導出はノート全文の走査になるため、
  // 変化していないノート(オブジェクト同一)は前回の行を使い回す。
  const rowCacheRef = useRef(new Map<string, NoteListRow>());
  const noteRows = useMemo<NoteListRow[]>(() => {
    const cache = rowCacheRef.current;
    const nextCache = new Map<string, NoteListRow>();
    const rows = notes.map((note) => {
      const cached = cache.get(note.id);
      if (cached && cached.note === note) {
        nextCache.set(note.id, cached);
        return cached;
      }
      const title = deriveNoteTitle(note.content);
      const plainText = extractPlainText(note.content);
      const row: NoteListRow = {
        note,
        title,
        preview: plainText || "本文なし",
        searchText: `${note.title} ${title} ${plainText}`.toLowerCase(),
      };
      nextCache.set(note.id, row);
      return row;
    });
    rowCacheRef.current = nextCache;
    return rows;
  }, [notes]);

  const filteredRows = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return noteRows;
    return noteRows.filter((row) => row.searchText.includes(normalized));
  }, [noteRows, query]);

  const selectedNote = notes.find((note) => note.id === selectedId) ?? null;
  const contextMenuNote = contextMenu
    ? notes.find((note) => note.id === contextMenu.noteId) ?? null
    : null;

  useEffect(() => {
    if (!contextMenu) return;
    if (notes.some((note) => note.id === contextMenu.noteId)) return;
    setContextMenu(null);
  }, [contextMenu, notes]);

  useEffect(() => {
    if (!contextMenu) return;

    function closeContextMenu() {
      setContextMenu(null);
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") closeContextMenu();
    }

    function handleContextMenu(event: MouseEvent) {
      if (!event.defaultPrevented) closeContextMenu();
    }

    window.addEventListener("click", closeContextMenu);
    window.addEventListener("blur", closeContextMenu);
    window.addEventListener("resize", closeContextMenu);
    window.addEventListener("scroll", closeContextMenu, true);
    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("contextmenu", handleContextMenu);
    return () => {
      window.removeEventListener("click", closeContextMenu);
      window.removeEventListener("blur", closeContextMenu);
      window.removeEventListener("resize", closeContextMenu);
      window.removeEventListener("scroll", closeContextMenu, true);
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("contextmenu", handleContextMenu);
    };
  }, [contextMenu]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.repeat) return;
      if (!event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
        return;
      }

      const key = event.key.toLowerCase();
      if (key === "n") {
        event.preventDefault();
        createAndSelectNote();
        return;
      }

      if (event.key !== "Backspace" && event.key !== "Delete") {
        return;
      }

      if (isEditableShortcutTarget(event.target)) return;

      const targetId =
        focusedNoteId && notes.some((note) => note.id === focusedNoteId)
          ? focusedNoteId
          : selectedId;
      if (!targetId) return;

      event.preventDefault();
      deleteNote(targetId);
    }

    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [createAndSelectNote, deleteNote, focusedNoteId, notes, selectedId]);

  const openNoteContextMenu = useCallback(
    (note: StickyNote, event: ReactMouseEvent<HTMLDivElement>) => {
      event.preventDefault();
      selectNote(note.id);
      setContextMenu({
        noteId: note.id,
        ...clampContextMenuPosition(event.clientX, event.clientY),
      });
    },
    [selectNote],
  );

  const focusNote = useCallback((id: string) => setFocusedNoteId(id), []);

  const togglePinnedNote = useCallback(
    (note: StickyNote) => {
      if (note.pinned) {
        void hidePinned(note.id);
      } else {
        void showPinned(note.id);
      }
    },
    [hidePinned, showPinned],
  );

  return (
    <div className="flex min-h-0 flex-1 bg-surface">
      <aside className="flex h-full w-[292px] shrink-0 flex-col border-r border-edge bg-canvas">
        <div className="shrink-0 px-4 pt-3.5 pb-3">
          <div className="flex items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2">
              <StickyNoteIcon size={16} className="shrink-0 text-ink-faint" />
              <h1 className="truncate text-[13px] font-semibold text-ink">
                メモ
              </h1>
            </div>
            <button
              type="button"
              title="新規メモ（⌘N）"
              aria-label="新規メモ"
              onClick={createAndSelectNote}
              className="grid size-7 place-items-center rounded-md bg-accent text-white transition-colors duration-100 focus-ring hover:bg-accent-deep active:scale-95"
            >
              <Plus size={15} />
            </button>
          </div>
          <div className="mt-3 flex h-8 items-center gap-2 rounded-md border border-edge bg-surface px-2 transition-[border-color,box-shadow] duration-100 focus-within:border-accent focus-within:ring-2 focus-within:ring-accent/25">
            <Search size={13} className="shrink-0 text-ink-faint" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              className="min-w-0 flex-1 bg-transparent text-[12px] text-ink placeholder:text-ink-faint focus:outline-none"
              placeholder="検索"
            />
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
          {filteredRows.map(({ note, title, preview }) => (
            <NoteListItem
              key={note.id}
              note={note}
              title={title}
              preview={preview}
              selected={note.id === selectedId}
              onSelect={selectNote}
              onFocus={focusNote}
              onTogglePinned={togglePinnedNote}
              onOpenContextMenu={openNoteContextMenu}
            />
          ))}
          {filteredRows.length === 0 ? (
            <p className="px-3 py-8 text-center text-xs text-ink-faint">
              メモがありません
            </p>
          ) : null}
        </div>

      </aside>

      <main className="min-w-0 flex-1">
        {selectedNote ? (
          <div
            className="h-full min-h-0"
            onFocusCapture={() => setFocusedNoteId(selectedNote.id)}
          >
            <NoteEditorPanel
              note={selectedNote}
              autoFocusEditor={editorFocusNoteId === selectedNote.id}
              onPatch={(patch) => patchNote(selectedNote.id, patch)}
              onDelete={() => deleteNote(selectedNote.id)}
              onShowPinned={() => void showPinned(selectedNote.id)}
              onHidePinned={() => void hidePinned(selectedNote.id)}
              onEditorAutoFocused={() => setEditorFocusNoteId(null)}
            />
          </div>
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-ink-faint">
            メモを選択
          </div>
        )}
      </main>

      {contextMenu && contextMenuNote ? (
        <div
          role="menu"
          aria-label="メモの操作"
          onContextMenu={(event) => event.preventDefault()}
          className="fixed z-50 w-32 rounded-lg bg-raised py-1 shadow-pop animate-pop-in"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setContextMenu(null);
              deleteNote(contextMenuNote.id);
            }}
            className="flex w-full items-center gap-2 px-3 py-2 text-left text-xs text-danger transition-colors duration-100 hover:bg-danger-soft"
          >
            <Trash2 size={14} />
            削除
          </button>
        </div>
      ) : null}
    </div>
  );
}

export function StickyNoteWindow({ noteId }: { noteId: string }) {
  const { notes, loaded, patchNote, removeNote, hidePinned } = useStickyNotes({
    createWhenEmpty: false,
  });
  const note = notes.find((candidate) => candidate.id === noteId) ?? null;

  useEffect(() => {
    document.body.classList.add("sticky-window");
    return () => document.body.classList.remove("sticky-window");
  }, []);

  if (!loaded) {
    return <div className="h-full bg-canvas" />;
  }

  if (!note) {
    return (
      <div className="flex h-full items-center justify-center bg-canvas text-sm text-ink-faint">
        メモが見つかりません
      </div>
    );
  }

  return (
    <div
      className={`relative h-full min-h-0 ${noteColorClass(note.color, true)}`}
    >
      <WindowDragRegion
        className={`absolute inset-x-0 top-0 z-10 h-[28px] ${noteColorClass(
          note.color,
          true,
        )}`}
      />
      <div className="h-full min-h-0 pt-[28px]">
        <NoteEditorPanel
          note={note}
          showToolbar={false}
          onPatch={(patch) => patchNote(note.id, patch)}
          onDelete={() => void removeNote(note.id)}
          onShowPinned={() => undefined}
          onHidePinned={() => void hidePinned(note.id)}
        />
      </div>
    </div>
  );
}

function NoteEditorPanel({
  note,
  onPatch,
  onDelete,
  onShowPinned,
  onHidePinned,
  onEditorAutoFocused,
  showToolbar = true,
  autoFocusEditor = false,
}: {
  note: StickyNote;
  onPatch: (patch: Partial<Pick<StickyNote, "content" | "color">>) => void;
  onDelete: () => void;
  onShowPinned: () => void;
  onHidePinned: () => void;
  onEditorAutoFocused?: () => void;
  showToolbar?: boolean;
  autoFocusEditor?: boolean;
}) {
  const content = useMemo(() => normalizeRichTextNode(note.content), [note.content]);

  return (
    <div
      className={`flex h-full min-h-0 flex-col ${noteColorClass(
        note.color,
        true,
      )}`}
    >
      {showToolbar ? (
        <div className="shrink-0 border-b border-edge px-4 pt-3 pb-3">
          <div className="flex items-center gap-2">
            <div className="flex min-w-0 flex-1 items-center gap-1.5">
              {noteColorPresets.map((preset) => (
                <button
                  key={preset.id}
                  type="button"
                  title={preset.label}
                  aria-label={preset.label}
                  onClick={() => onPatch({ color: preset.id })}
                  className={`grid size-[18px] place-items-center rounded-full transition-[box-shadow,transform] duration-100 focus-ring active:scale-90 ${
                    note.color === preset.id
                      ? "shadow-[0_0_0_1.5px_var(--surface),0_0_0_3px_var(--ink)]"
                      : "shadow-[inset_0_0_0_1px_var(--edge-strong)] hover:scale-110"
                  }`}
                  style={{ backgroundColor: preset.swatch }}
                >
                  {note.color === preset.id ? <Check size={11} /> : null}
                </button>
              ))}
            </div>
            <div className="flex shrink-0 items-center gap-1">
              <button
                type="button"
                title={note.pinned ? "最前面を解除" : "最前面に表示"}
                aria-label={note.pinned ? "最前面を解除" : "最前面に表示"}
                onClick={note.pinned ? onHidePinned : onShowPinned}
                className={`grid size-7 place-items-center rounded-md transition-colors duration-100 focus-ring ${
                  note.pinned
                    ? "bg-ink text-canvas"
                    : "text-ink-mid hover:bg-hover hover:text-ink"
                }`}
              >
                {note.pinned ? <PinOff size={14} /> : <Pin size={14} />}
              </button>
              <button
                type="button"
                title="削除"
                aria-label="削除"
                onClick={onDelete}
                className="grid size-7 place-items-center rounded-md text-ink-mid transition-colors duration-100 focus-ring hover:bg-danger hover:text-white"
              >
                <Trash2 size={14} />
              </button>
            </div>
          </div>
        </div>
      ) : null}
      <RichNoteEditor
        key={note.id}
        noteId={note.id}
        content={content}
        autoFocus={autoFocusEditor}
        onChange={(next) => onPatch({ content: next as Record<string, unknown> })}
        onAutoFocusComplete={onEditorAutoFocused}
      />
    </div>
  );
}

const NoteListItem = memo(function NoteListItem({
  note,
  title,
  preview,
  selected,
  onSelect,
  onFocus,
  onTogglePinned,
  onOpenContextMenu,
}: {
  note: StickyNote;
  title: string;
  preview: string;
  selected: boolean;
  onSelect: (id: string) => void;
  onFocus: (id: string) => void;
  onTogglePinned: (note: StickyNote) => void;
  onOpenContextMenu: (
    note: StickyNote,
    event: ReactMouseEvent<HTMLDivElement>,
  ) => void;
}) {
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => onSelect(note.id)}
      onFocus={() => onFocus(note.id)}
      onDoubleClick={(event) => {
        event.preventDefault();
        onTogglePinned(note);
      }}
      onContextMenu={(event) => onOpenContextMenu(note, event)}
      onKeyDown={(event) => {
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        onSelect(note.id);
      }}
      title={note.pinned ? "固定中" : "未固定"}
      className={`mb-0.5 flex w-full cursor-pointer items-start gap-2.5 rounded-lg px-3 py-2 text-left transition-colors duration-100 focus-ring ${
        selected ? "bg-accent-soft" : "hover:bg-hover"
      }`}
    >
      <span
        className={`mt-1 size-2.5 shrink-0 rounded-full ${noteColorClass(
          note.color,
        )}`}
      />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px] font-medium text-ink">
          {title}
        </span>
        <span className="mt-0.5 block truncate text-[11px] text-ink-mid">
          {preview}
        </span>
        <span className="mt-1 block text-[10px] text-ink-faint">
          {formatDate(note.updatedAt)}
        </span>
      </span>
      {note.pinned ? (
        <span
          role="img"
          title="固定中"
          aria-label="固定中"
          className="mt-0.5 grid size-5 shrink-0 place-items-center text-ink-mid"
        >
          <Pin size={12} fill="currentColor" />
        </span>
      ) : null}
    </div>
  );
});

function useStickyNotes({ createWhenEmpty }: { createWhenEmpty: boolean }) {
  const [notes, setNotes] = useState<StickyNote[]>([]);
  const [loaded, setLoaded] = useState(false);
  const creatingRef = useRef(false);
  const deletingIdsRef = useRef(new Set<string>());
  const saveTimersRef = useRef(new Map<string, number>());

  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const loadedNotes = await getStickyNotes();
        if (cancelled) return;
        if (loadedNotes.length === 0 && createWhenEmpty && !creatingRef.current) {
          creatingRef.current = true;
          try {
            const note = await createStickyNote();
            if (!cancelled) setNotes([note]);
          } finally {
            creatingRef.current = false;
          }
        } else {
          setNotes(loadedNotes);
        }
      } catch (error) {
        console.error("[enja] sticky notes load failed", error);
      } finally {
        if (!cancelled) setLoaded(true);
      }
    }
    void load();
    return () => {
      cancelled = true;
    };
  }, [createWhenEmpty]);

  useEffect(() => {
    const unlistenPromise = listen<StickyNote[]>(
      "sticky-notes-changed",
      (event) => {
        setNotes(event.payload);
        setLoaded(true);
      },
    );
    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    return () => {
      for (const timer of saveTimersRef.current.values()) {
        window.clearTimeout(timer);
      }
      saveTimersRef.current.clear();
    };
  }, []);

  const scheduleSave = useCallback((note: StickyNote) => {
    const existing = saveTimersRef.current.get(note.id);
    if (existing) window.clearTimeout(existing);
    const timer = window.setTimeout(() => {
      saveTimersRef.current.delete(note.id);
      void updateStickyNote({
        id: note.id,
        title: note.title,
        content: note.content,
        color: note.color,
      }).catch((error) => {
        console.error("[enja] sticky note save failed", error);
      });
    }, 450);
    saveTimersRef.current.set(note.id, timer);
  }, []);

  const patchNote = useCallback(
    (id: string, patch: Partial<Pick<StickyNote, "content" | "color">>) => {
      setNotes((current) => {
        const target = current.find((note) => note.id === id);
        if (!target) return current;
        const nextContent = patch.content ?? target.content;
        const nextColor = patch.color ?? target.color;
        const contentChanged =
          patch.content !== undefined &&
          serializeRichTextNode(nextContent) !==
            serializeRichTextNode(target.content);
        const colorChanged = patch.color !== undefined && nextColor !== target.color;
        if (!contentChanged && !colorChanged) return current;

        const next: StickyNote = {
          ...target,
          content: nextContent,
          color: nextColor,
          title: deriveNoteTitle(nextContent),
          updatedAt: contentChanged ? Date.now() : target.updatedAt,
        };
        scheduleSave(next);
        const nextNotes = current.map((note) => (note.id === id ? next : note));
        return contentChanged ? sortNotesByUpdatedAt(nextNotes) : nextNotes;
      });
    },
    [scheduleSave],
  );

  const createNote = useCallback(async () => {
    const note = await createStickyNote();
    setNotes((current) => {
      if (current.some((candidate) => candidate.id === note.id)) return current;
      return [note, ...current];
    });
    return note;
  }, []);

  const removeNote = useCallback(async (id: string) => {
    if (deletingIdsRef.current.has(id)) return;
    deletingIdsRef.current.add(id);
    const pendingSave = saveTimersRef.current.get(id);
    if (pendingSave) {
      window.clearTimeout(pendingSave);
      saveTimersRef.current.delete(id);
    }

    try {
      await deleteStickyNote(id);
      setNotes((current) => current.filter((note) => note.id !== id));
    } catch (error) {
      console.error("[enja] sticky note delete failed", error);
    } finally {
      deletingIdsRef.current.delete(id);
    }
  }, []);

  const showPinned = useCallback(async (id: string) => {
    try {
      await showStickyNoteWindow(id);
    } catch (error) {
      console.error("[enja] sticky note show failed", error);
    }
  }, []);

  const hidePinned = useCallback(async (id: string) => {
    try {
      await hideStickyNoteWindow(id);
    } catch (error) {
      console.error("[enja] sticky note hide failed", error);
    }
  }, []);

  return {
    notes,
    loaded,
    patchNote,
    createNote,
    removeNote,
    showPinned,
    hidePinned,
  };
}

function formatDate(value: number) {
  return new Intl.DateTimeFormat("ja-JP", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function sortNotesByUpdatedAt(notes: StickyNote[]) {
  return [...notes].sort((a, b) => b.updatedAt - a.updatedAt);
}

function clampContextMenuPosition(x: number, y: number) {
  const maxX = Math.max(
    NOTE_CONTEXT_MENU_MARGIN,
    window.innerWidth - NOTE_CONTEXT_MENU_WIDTH - NOTE_CONTEXT_MENU_MARGIN,
  );
  const maxY = Math.max(
    NOTE_CONTEXT_MENU_MARGIN,
    window.innerHeight - NOTE_CONTEXT_MENU_HEIGHT - NOTE_CONTEXT_MENU_MARGIN,
  );

  return {
    x: Math.min(Math.max(x, NOTE_CONTEXT_MENU_MARGIN), maxX),
    y: Math.min(Math.max(y, NOTE_CONTEXT_MENU_MARGIN), maxY),
  };
}

function isEditableShortcutTarget(target: EventTarget | null) {
  if (!(target instanceof Element)) return false;
  const editable = target.closest(
    "input, textarea, select, [contenteditable='true'], .ProseMirror",
  );
  if (!editable) return false;
  if (
    editable instanceof HTMLInputElement ||
    editable instanceof HTMLTextAreaElement ||
    editable instanceof HTMLSelectElement
  ) {
    return !editable.disabled;
  }
  return true;
}
