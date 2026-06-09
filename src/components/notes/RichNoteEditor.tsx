import { useEffect, useRef, type MouseEvent as ReactMouseEvent } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { Editor } from "@tiptap/react";
import { EditorContent, useEditor } from "@tiptap/react";
import { NodeSelection } from "@tiptap/pm/state";
import { marked } from "marked";
import StarterKit from "@tiptap/starter-kit";
import Image from "@tiptap/extension-image";
import Link from "@tiptap/extension-link";
import Placeholder from "@tiptap/extension-placeholder";
import Typography from "@tiptap/extension-typography";
import { saveStickyNoteImage } from "../../lib/commands";
import {
  normalizeRichTextNode,
  noteToMarkdown,
  type RichTextNode,
} from "../../lib/stickyNotes";

type RichNoteEditorProps = {
  noteId: string;
  content: RichTextNode;
  autoFocus?: boolean;
  onChange: (content: RichTextNode) => void;
  onAutoFocusComplete?: () => void;
};

const MAX_IMAGE_FILE_BYTES = 20 * 1024 * 1024;

export function RichNoteEditor({
  noteId,
  content,
  autoFocus = false,
  onChange,
  onAutoFocusComplete,
}: RichNoteEditorProps) {
  const editorRef = useRef<Editor | null>(null);
  const currentNoteIdRef = useRef(noteId);
  const lastNoteIdRef = useRef(noteId);
  const onChangeRef = useRef(onChange);
  const onAutoFocusCompleteRef = useRef(onAutoFocusComplete);
  const lastContentRef = useRef<string | null>(null);
  currentNoteIdRef.current = noteId;
  onChangeRef.current = onChange;
  onAutoFocusCompleteRef.current = onAutoFocusComplete;
  if (lastContentRef.current === null) {
    lastContentRef.current = JSON.stringify(normalizeRichTextNode(content));
  }

  async function insertImageFile(file: File) {
    if (!file.type.startsWith("image/")) return;
    if (file.size > MAX_IMAGE_FILE_BYTES) {
      console.error("[enja] sticky note image is too large", file.name);
      return;
    }
    const dataBase64 = await fileToDataUrl(file);
    const saved = await saveStickyNoteImage({
      noteId: currentNoteIdRef.current,
      mimeType: file.type,
      dataBase64,
      fileName: file.name || null,
    });
    editorRef.current
      ?.chain()
      .focus()
      .setImage({
        src: convertFileSrc(saved.path),
        alt: saved.fileName,
        title: saved.path,
      })
      .run();
  }

  const editor = useEditor({
    immediatelyRender: true,
    extensions: [
      StarterKit.configure({
        link: false,
      }),
      Link.configure({
        autolink: true,
        openOnClick: false,
        linkOnPaste: true,
      }),
      Image.configure({
        allowBase64: false,
        resize: {
          enabled: true,
          minWidth: 96,
          alwaysPreserveAspectRatio: true,
        },
        HTMLAttributes: {
          class: "note-editor-image",
        },
      }),
      Placeholder.configure({
        placeholder: "メモを書く",
      }),
      Typography,
    ],
    content: normalizeRichTextNode(content) as never,
    editorProps: {
      attributes: {
        class:
          "note-editor-prose min-h-full px-5 py-4 text-[14px] leading-relaxed text-neutral-800 focus:outline-none",
      },
      handlePaste: (_view, event) => {
        const files = imageFilesFromClipboard(event);
        if (!files.length) return false;
        event.preventDefault();
        for (const file of files) {
          void insertImageFile(file).catch((error) => {
            console.error("[enja] sticky note image paste failed", error);
          });
        }
        return true;
      },
      handleDrop: (_view, event) => {
        const files = Array.from(event.dataTransfer?.files ?? []).filter((file) =>
          file.type.startsWith("image/"),
        );
        if (!files.length) return false;
        event.preventDefault();
        for (const file of files) {
          void insertImageFile(file).catch((error) => {
            console.error("[enja] sticky note image drop failed", error);
          });
        }
        return true;
      },
      handleDOMEvents: {
        paste: (_view, event) => {
          const files = imageFilesFromClipboard(event);
          if (files.length) return false;

          const text = event.clipboardData?.getData("text/plain") ?? "";
          if (!looksLikeMarkdown(text)) return false;

          event.preventDefault();
          editorRef.current
            ?.chain()
            .focus()
            .insertContent(markdownToEditorHtml(text))
            .run();
          return true;
        },
        copy: (_view, event) => {
          const editor = editorRef.current;
          if (!editor) return false;

          const selection = editor.state.selection;
          if (selection instanceof NodeSelection && selection.node.type.name === "image") {
            event.preventDefault();
            void copyImageNode(selection.node.attrs);
            return true;
          }

          const markdown = selectionToMarkdown(editor);
          if (!markdown) {
            return false;
          }
          event.preventDefault();
          if (event.clipboardData) {
            event.clipboardData.setData("text/plain", markdown);
          } else {
            void navigator.clipboard.writeText(markdown);
          }
          return true;
        },
      },
    },
    onUpdate: ({ editor }) => {
      const next = editor.getJSON() as RichTextNode;
      lastNoteIdRef.current = currentNoteIdRef.current;
      lastContentRef.current = JSON.stringify(next);
      onChangeRef.current(next);
    },
  });

  function focusEditorFromBlankArea(event: ReactMouseEvent<HTMLDivElement>) {
    const target = event.target;
    if (!(target instanceof Element) || target.closest(".ProseMirror")) return;
    event.preventDefault();
    editor?.chain().focus("end").run();
  }

  useEffect(() => {
    editorRef.current = editor;
    return () => {
      if (editorRef.current === editor) editorRef.current = null;
    };
  }, [editor]);

  useEffect(() => {
    if (!editor) return;
    const normalized = normalizeRichTextNode(content);
    const serialized = JSON.stringify(normalized);
    const noteChanged = noteId !== lastNoteIdRef.current;
    if (!noteChanged && serialized === lastContentRef.current) return;
    if (!noteChanged && editor.isFocused) return;
    lastNoteIdRef.current = noteId;
    lastContentRef.current = serialized;
    editor.commands.setContent(normalized as never, { emitUpdate: false });
  }, [content, editor, noteId]);

  useEffect(() => {
    if (!editor || !autoFocus) return;

    const frame = window.requestAnimationFrame(() => {
      editor.chain().focus("end").run();
      onAutoFocusCompleteRef.current?.();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [autoFocus, editor]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div
        className="min-h-0 flex-1 overflow-y-auto"
        onMouseDown={focusEditorFromBlankArea}
      >
        <EditorContent editor={editor} className="min-h-full" />
      </div>
    </div>
  );
}

function imageFilesFromClipboard(event: ClipboardEvent) {
  return Array.from(event.clipboardData?.items ?? [])
    .filter((item) => item.kind === "file" && item.type.startsWith("image/"))
    .map((item) => item.getAsFile())
    .filter((file): file is File => Boolean(file));
}

function fileToDataUrl(file: File) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result ?? ""));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

function selectionToMarkdown(editor: Editor) {
  const selection = editor.state.selection;
  if (selection.empty) return null;

  const slice = selection.content();
  const content = (
    slice.content as unknown as { toJSON?: () => RichTextNode[] }
  ).toJSON?.();
  if (!Array.isArray(content)) return null;

  return noteToMarkdown({ type: "doc", content });
}

function looksLikeMarkdown(value: string) {
  const text = value.trim();
  if (!text) return false;
  return /(^|\n)\s{0,3}#{1,6}\s+\S/.test(text)
    || /(^|\n)\s{0,3}[-*+]\s+\S/.test(text)
    || /(^|\n)\s{0,3}[・•●]\s*\S/.test(text)
    || /(^|\n)\s{0,3}\d+[.)]\s+\S/.test(text)
    || /(^|\n)\s{0,3}>\s+\S/.test(text)
    || /(^|\n)\s{0,3}```/.test(text)
    || /!\[[^\]]*]\([^)]+/.test(text)
    || /\[[^\]]+]\([^)]+/.test(text)
    || /(\*\*|__|~~|`)[^\n]+(\*\*|__|~~|`)/.test(text);
}

function markdownToEditorHtml(markdown: string) {
  const normalized = preserveBlankParagraphs(
    normalizeMarkdownForPaste(markdown),
  );
  const html = marked(normalized, {
    async: false,
    breaks: true,
    gfm: true,
  });
  return sanitizeMarkdownHtml(html);
}

function normalizeMarkdownForPaste(markdown: string) {
  return markdown.replace(
    /^(\s*)[・•●]\s*/gm,
    "$1- ",
  );
}

function preserveBlankParagraphs(markdown: string) {
  return markdown.replace(/\n{3,}/g, (match) => {
    const emptyParagraphs = Math.max(1, match.length - 2);
    return `\n\n${"<p><br></p>\n\n".repeat(emptyParagraphs)}`;
  });
}

function sanitizeMarkdownHtml(html: string) {
  const template = document.createElement("template");
  template.innerHTML = html;

  for (const element of Array.from(
    template.content.querySelectorAll("script,style,iframe,object,embed"),
  )) {
    element.remove();
  }

  for (const element of Array.from(template.content.querySelectorAll("*"))) {
    for (const attribute of Array.from(element.attributes)) {
      const name = attribute.name.toLowerCase();
      const value = attribute.value.trim();
      if (name.startsWith("on")) {
        element.removeAttribute(attribute.name);
      }
      if (
        (name === "href" || name === "src") &&
        /^(?:javascript|data:text\/html)/i.test(value)
      ) {
        element.removeAttribute(attribute.name);
      }
    }

    if (element instanceof HTMLImageElement) {
      normalizeMarkdownImageElement(element);
    }
  }

  return template.innerHTML;
}

function normalizeMarkdownImageElement(element: HTMLImageElement) {
  const src = element.getAttribute("src") ?? "";
  if (!src || isBrowserUrl(src) || src.startsWith("asset:")) return;
  element.setAttribute("title", src);
  element.setAttribute("src", convertFileSrc(src));
}

function isBrowserUrl(value: string) {
  return /^(?:https?:|data:image\/|blob:)/i.test(value);
}

async function copyImageNode(attrs: Record<string, unknown>) {
  const src = String(attrs.src ?? "");
  const path = String(attrs.title ?? "");
  if (!src) return;

  try {
    if (!("ClipboardItem" in window) || !navigator.clipboard.write) {
      throw new Error("image clipboard is not available");
    }
    const response = await fetch(src);
    const blob = await response.blob();
    const mimeType = blob.type || mimeTypeFromPath(path) || "image/png";
    await navigator.clipboard.write([
      new ClipboardItem({
        [mimeType]: blob,
      }),
    ]);
  } catch {
    if (path) {
      await navigator.clipboard.writeText(path);
    }
  }
}

function mimeTypeFromPath(path: string) {
  const lower = path.toLowerCase();
  if (lower.endsWith(".jpg") || lower.endsWith(".jpeg")) return "image/jpeg";
  if (lower.endsWith(".gif")) return "image/gif";
  if (lower.endsWith(".webp")) return "image/webp";
  if (lower.endsWith(".png")) return "image/png";
  return null;
}
