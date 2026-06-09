import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Check,
  Languages,
  Pin,
  PinOff,
  Plus,
  Search,
  Settings,
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
} from "../../lib/stickyNotes";
import { useAppStore } from "../../stores/useAppStore";
import type { StickyNote } from "../../types";
import { RichNoteEditor } from "./RichNoteEditor";

type NoteContextMenuState = {
  noteId: string;
  x: number;
  y: number;
} | null;

const NOTE_CONTEXT_MENU_MARGIN = 8;
const NOTE_CONTEXT_MENU_WIDTH = 132;
const NOTE_CONTEXT_MENU_HEIGHT = 42;

export function NotesView() {
  const setView = useAppStore((s) => s.setView);
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
  const [query, setQuery] = useState("");
  const [contextMenu, setContextMenu] = useState<NoteContextMenuState>(null);

  function selectNote(id: string | null) {
    setSelectedId(id);
    setFocusedNoteId(id);
  }

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

  const filteredNotes = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return notes;
    return notes.filter((note) => {
      const haystack = `${deriveNoteTitle(note.content)} ${extractPlainText(
        note.content,
      )}`.toLowerCase();
      return haystack.includes(normalized);
    });
  }, [notes, query]);

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
        setContextMenu(null);
        void createNote().then((note) => selectNote(note.id));
        return;
      }

      if (event.key !== "Backspace" && event.key !== "Delete") {
        return;
      }

      const targetId =
        focusedNoteId && notes.some((note) => note.id === focusedNoteId)
          ? focusedNoteId
          : selectedId;
      if (!targetId) return;

      event.preventDefault();
      setContextMenu(null);

      const nextSelectedId =
        selectedId === targetId
          ? notes.find((note) => note.id !== targetId)?.id ?? null
          : selectedId;
      setSelectedId(nextSelectedId);
      setFocusedNoteId(nextSelectedId);
      void removeNote(targetId);
    }

    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [createNote, focusedNoteId, notes, removeNote, selectedId]);

  function openNoteContextMenu(
    note: StickyNote,
    event: ReactMouseEvent<HTMLDivElement>,
  ) {
    event.preventDefault();
    selectNote(note.id);
    setContextMenu({
      noteId: note.id,
      ...clampContextMenuPosition(event.clientX, event.clientY),
    });
  }

  return (
    <div className="flex min-h-0 flex-1 bg-white">
      <aside className="flex h-full w-[292px] shrink-0 flex-col border-r border-neutral-200 bg-neutral-50">
        <div className="shrink-0 border-b border-neutral-200 px-4 pt-3 pb-3">
          <div className="flex items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2">
              <StickyNoteIcon size={18} className="shrink-0 text-neutral-500" />
              <h1 className="truncate text-sm font-semibold text-neutral-800">
                メモ
              </h1>
            </div>
            <button
              type="button"
              title="新規メモ"
              aria-label="新規メモ"
              onClick={() => {
                void createNote().then((note) => selectNote(note.id));
              }}
              className="grid size-8 place-items-center rounded-md bg-neutral-900 text-white shadow-sm transition-colors hover:bg-neutral-700"
            >
              <Plus size={16} />
            </button>
          </div>
          <div className="mt-3 flex h-8 items-center gap-2 rounded-md border border-neutral-200 bg-white px-2">
            <Search size={14} className="shrink-0 text-neutral-400" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              className="min-w-0 flex-1 bg-transparent text-[12px] text-neutral-700 placeholder:text-neutral-400 focus:outline-none"
              placeholder="検索"
            />
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-2 py-2">
          {filteredNotes.map((note) => (
            <NoteListItem
              key={note.id}
              note={note}
              selected={note.id === selectedId}
              onSelect={() => selectNote(note.id)}
              onFocus={() => setFocusedNoteId(note.id)}
              onTogglePinned={() =>
                note.pinned ? void hidePinned(note.id) : void showPinned(note.id)
              }
              onOpenContextMenu={(event) => openNoteContextMenu(note, event)}
            />
          ))}
          {filteredNotes.length === 0 ? (
            <p className="px-3 py-8 text-center text-xs text-neutral-400">
              メモがありません
            </p>
          ) : null}
        </div>

        <div className="flex shrink-0 items-center gap-1 border-t border-neutral-200 px-3 py-2">
          <button
            type="button"
            title="翻訳"
            aria-label="翻訳"
            onClick={() => setView("translation")}
            className="grid size-8 place-items-center rounded-md text-neutral-400 transition-colors hover:bg-neutral-200 hover:text-neutral-700"
          >
            <Languages size={16} />
          </button>
          <button
            type="button"
            title="設定"
            aria-label="設定"
            onClick={() => setView("settings")}
            className="grid size-8 place-items-center rounded-md text-neutral-400 transition-colors hover:bg-neutral-200 hover:text-neutral-700"
          >
            <Settings size={16} />
          </button>
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
              onPatch={(patch) => patchNote(selectedNote.id, patch)}
              onDelete={() => void removeNote(selectedNote.id)}
              onShowPinned={() => void showPinned(selectedNote.id)}
              onHidePinned={() => void hidePinned(selectedNote.id)}
            />
          </div>
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-neutral-400">
            メモを選択
          </div>
        )}
      </main>

      {contextMenu && contextMenuNote ? (
        <div
          role="menu"
          aria-label="メモの操作"
          onContextMenu={(event) => event.preventDefault()}
          className="fixed z-50 w-32 rounded-md border border-neutral-200 bg-white py-1 shadow-lg"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setContextMenu(null);
              void removeNote(contextMenuNote.id);
            }}
            className="flex w-full items-center gap-2 px-3 py-2 text-left text-xs text-red-600 transition-colors hover:bg-red-50"
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
    return <div className="h-full bg-neutral-50" />;
  }

  if (!note) {
    return (
      <div className="flex h-full items-center justify-center bg-neutral-50 text-sm text-neutral-400">
        メモが見つかりません
      </div>
    );
  }

  return (
    <NoteEditorPanel
      note={note}
      showToolbar={false}
      onPatch={(patch) => patchNote(note.id, patch)}
      onDelete={() => void removeNote(note.id)}
      onShowPinned={() => undefined}
      onHidePinned={() => void hidePinned(note.id)}
    />
  );
}

function NoteEditorPanel({
  note,
  onPatch,
  onDelete,
  onShowPinned,
  onHidePinned,
  showToolbar = true,
}: {
  note: StickyNote;
  onPatch: (patch: Partial<Pick<StickyNote, "content" | "color">>) => void;
  onDelete: () => void;
  onShowPinned: () => void;
  onHidePinned: () => void;
  showToolbar?: boolean;
}) {
  const content = normalizeRichTextNode(note.content);

  return (
    <div
      className={`flex h-full min-h-0 flex-col ${noteColorClass(
        note.color,
        true,
      )}`}
    >
      {showToolbar ? (
        <div className="shrink-0 border-b border-black/5 px-4 pt-3 pb-3">
          <div className="flex items-center gap-2">
            <div className="flex min-w-0 flex-1 items-center gap-1">
              {noteColorPresets.map((preset) => (
                <button
                  key={preset.id}
                  type="button"
                  title={preset.label}
                  aria-label={preset.label}
                  onClick={() => onPatch({ color: preset.id })}
                  className={`grid size-6 place-items-center rounded-full border transition ${
                    note.color === preset.id
                      ? "border-neutral-900"
                      : "border-black/10 hover:border-neutral-500"
                  }`}
                  style={{ backgroundColor: preset.swatch }}
                >
                  {note.color === preset.id ? <Check size={12} /> : null}
                </button>
              ))}
            </div>
            <div className="flex shrink-0 items-center gap-1">
              <button
                type="button"
                title={note.pinned ? "最前面を解除" : "最前面に表示"}
                aria-label={note.pinned ? "最前面を解除" : "最前面に表示"}
                onClick={note.pinned ? onHidePinned : onShowPinned}
                className={`grid size-8 place-items-center rounded-md transition-colors ${
                  note.pinned
                    ? "bg-neutral-900 text-white"
                    : "text-neutral-500 hover:bg-black/5 hover:text-neutral-800"
                }`}
              >
                {note.pinned ? <PinOff size={15} /> : <Pin size={15} />}
              </button>
              <button
                type="button"
                title="削除"
                aria-label="削除"
                onClick={onDelete}
                className="grid size-8 place-items-center rounded-md text-neutral-500 transition-colors hover:bg-red-500 hover:text-white"
              >
                <Trash2 size={15} />
              </button>
            </div>
          </div>
        </div>
      ) : null}
      <RichNoteEditor
        noteId={note.id}
        content={content}
        onChange={(next) => onPatch({ content: next as Record<string, unknown> })}
      />
    </div>
  );
}

function NoteListItem({
  note,
  selected,
  onSelect,
  onFocus,
  onTogglePinned,
  onOpenContextMenu,
}: {
  note: StickyNote;
  selected: boolean;
  onSelect: () => void;
  onFocus: () => void;
  onTogglePinned: () => void;
  onOpenContextMenu: (event: ReactMouseEvent<HTMLDivElement>) => void;
}) {
  const preview = extractPlainText(note.content) || "本文なし";
  const title = deriveNoteTitle(note.content);
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onSelect}
      onFocus={onFocus}
      onDoubleClick={(event) => {
        event.preventDefault();
        onTogglePinned();
      }}
      onContextMenu={onOpenContextMenu}
      onKeyDown={(event) => {
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        onSelect();
      }}
      title={note.pinned ? "固定中" : "未固定"}
      className={`mb-1 flex w-full cursor-pointer items-start gap-3 rounded-md border px-3 py-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-neutral-300 ${
        selected
          ? "border-neutral-300 bg-white shadow-sm"
          : "border-transparent hover:bg-white/80"
      }`}
    >
      <span
        className={`mt-0.5 size-3 shrink-0 rounded-full ${noteColorClass(
          note.color,
        )}`}
      />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px] font-medium text-neutral-800">
          {title}
        </span>
        <span className="mt-0.5 block truncate text-[11px] text-neutral-500">
          {preview}
        </span>
        <span className="mt-1 block text-[10px] text-neutral-400">
          {formatDate(note.updatedAt)}
        </span>
      </span>
      <span
        role="img"
        title={note.pinned ? "固定中" : "未固定"}
        aria-label={note.pinned ? "固定中" : "未固定"}
        className={`mt-0.5 grid size-6 shrink-0 place-items-center rounded-md ${
          note.pinned ? "text-neutral-900" : "text-neutral-300"
        }`}
      >
        {note.pinned ? <Pin size={14} fill="currentColor" /> : <PinOff size={14} />}
      </span>
    </div>
  );
}

function useStickyNotes({ createWhenEmpty }: { createWhenEmpty: boolean }) {
  const [notes, setNotes] = useState<StickyNote[]>([]);
  const [loaded, setLoaded] = useState(false);
  const creatingRef = useRef(false);
  const deletingIdsRef = useRef(new Set<string>());
  const saveTimersRef = useRef(new Map<string, number>());

  useEffect(() => {
    let cancelled = false;
    async function load() {
      const loadedNotes = await getStickyNotes();
      if (cancelled) return;
      if (loadedNotes.length === 0 && createWhenEmpty && !creatingRef.current) {
        creatingRef.current = true;
        const note = await createStickyNote();
        if (!cancelled) setNotes([note]);
      } else {
        setNotes(loadedNotes);
      }
      if (!cancelled) setLoaded(true);
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

  function scheduleSave(note: StickyNote) {
    const existing = saveTimersRef.current.get(note.id);
    if (existing) window.clearTimeout(existing);
    const timer = window.setTimeout(() => {
      saveTimersRef.current.delete(note.id);
      void updateStickyNote({
        id: note.id,
        title: note.title,
        content: note.content,
        color: note.color,
      });
    }, 450);
    saveTimersRef.current.set(note.id, timer);
  }

  function patchNote(
    id: string,
    patch: Partial<Pick<StickyNote, "content" | "color">>,
  ) {
    setNotes((current) => {
      const target = current.find((note) => note.id === id);
      if (!target) return current;
      const nextContent = patch.content ?? target.content;
      const nextColor = patch.color ?? target.color;
      const contentChanged =
        patch.content !== undefined &&
        serializeNoteContent(nextContent) !== serializeNoteContent(target.content);
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
  }

  async function createNote() {
    const note = await createStickyNote();
    setNotes((current) => {
      if (current.some((candidate) => candidate.id === note.id)) return current;
      return [note, ...current];
    });
    return note;
  }

  async function removeNote(id: string) {
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
    } finally {
      deletingIdsRef.current.delete(id);
    }
  }

  async function showPinned(id: string) {
    await showStickyNoteWindow(id);
  }

  async function hidePinned(id: string) {
    await hideStickyNoteWindow(id);
  }

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

function serializeNoteContent(content: unknown) {
  return JSON.stringify(normalizeRichTextNode(content));
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
