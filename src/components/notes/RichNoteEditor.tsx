import { useEffect, useRef, type MouseEvent as ReactMouseEvent } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { Editor } from "@tiptap/react";
import { EditorContent, useEditor } from "@tiptap/react";
import { NodeSelection } from "@tiptap/pm/state";
import { DOMSerializer } from "@tiptap/pm/model";
import { marked } from "marked";
import StarterKit from "@tiptap/starter-kit";
import Image from "@tiptap/extension-image";
import Link from "@tiptap/extension-link";
import Placeholder from "@tiptap/extension-placeholder";
import TaskItem from "@tiptap/extension-task-item";
import TaskList from "@tiptap/extension-task-list";
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
  const shiftKeyRef = useRef(false);
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
      TaskList.configure({
        HTMLAttributes: {
          class: "note-editor-task-list",
        },
      }),
      TaskItem.configure({
        nested: true,
        HTMLAttributes: {
          class: "note-editor-task-item",
        },
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
        const files = Array.from(event.dataTransfer?.files ?? []);
        if (!files.length) return false;
        event.preventDefault();
        for (const file of files.filter((file) => file.type.startsWith("image/"))) {
          void insertImageFile(file).catch((error) => {
            console.error("[enja] sticky note image drop failed", error);
          });
        }
        return true;
      },
      handleDOMEvents: {
        paste: (view, event) => {
          const files = imageFilesFromClipboard(event);
          if (files.length) return false;

          const clipboard = event.clipboardData;
          if (!clipboard) return false;

          const text = clipboard.getData("text/plain");

          if (shiftKeyRef.current) {
            if (!text) return false;
            event.preventDefault();
            view.pasteText(text);
            return true;
          }

          const html = clipboard.getData("text/html");
          if (html.trim() && htmlHasRichStructure(html)) {
            event.preventDefault();
            view.pasteHTML(sanitizePastedHtml(html));
            return true;
          }

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
            const html = selectionToHtml(editor);
            if (html) {
              event.clipboardData.setData("text/html", html);
            }
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
    const updateShiftKey = (event: KeyboardEvent) => {
      shiftKeyRef.current = event.shiftKey;
    };
    window.addEventListener("keydown", updateShiftKey, true);
    window.addEventListener("keyup", updateShiftKey, true);
    return () => {
      window.removeEventListener("keydown", updateShiftKey, true);
      window.removeEventListener("keyup", updateShiftKey, true);
    };
  }, []);

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

function selectionToHtml(editor: Editor) {
  const selection = editor.state.selection;
  if (selection.empty) return null;

  const serializer = DOMSerializer.fromSchema(editor.schema);
  const container = document.createElement("div");
  container.appendChild(serializer.serializeFragment(selection.content().content));
  flattenTaskListsForExport(container);
  preserveEmptyParagraphs(container);
  return container.innerHTML || null;
}

// TipTapのタスク項目は <li><label><input></label><div><p>…</p></div></li> と
// 入れ子になっており、NotionなどはlabelとdivをBlock扱いして改行してしまう。
// GitHub形式の <li><input type="checkbox"> テキスト</li> にフラット化する
function flattenTaskListsForExport(root: HTMLElement) {
  for (const list of Array.from(
    root.querySelectorAll('ul[data-type="taskList"]'),
  )) {
    for (const item of directListItems(list)) {
      const checked = item.getAttribute("data-checked") === "true";
      item.querySelector(":scope > label")?.remove();

      const wrapper = item.querySelector(":scope > div");
      if (wrapper) {
        wrapper.replaceWith(...Array.from(wrapper.childNodes));
      }
      const blocks = Array.from(item.children).filter(
        (child) => !(child instanceof HTMLUListElement || child instanceof HTMLOListElement),
      );
      if (blocks.length === 1 && blocks[0] instanceof HTMLParagraphElement) {
        blocks[0].replaceWith(...Array.from(blocks[0].childNodes));
      }

      const checkbox = item.ownerDocument.createElement("input");
      checkbox.type = "checkbox";
      checkbox.disabled = true;
      if (checked) {
        checkbox.setAttribute("checked", "checked");
      }
      item.prepend(checkbox, " ");
    }
  }
}

// 空段落は <p></p> のままだと受け側エディタで行ごと消えるため、
// <p><br></p> にして空行として残す（貼り付け時のProseMirrorも同様）
function preserveEmptyParagraphs(root: ParentNode) {
  for (const paragraph of Array.from(root.querySelectorAll("p"))) {
    if (!paragraph.childNodes.length || (
      !paragraph.children.length && !paragraph.textContent?.trim()
    )) {
      paragraph.replaceChildren(paragraph.ownerDocument.createElement("br"));
    }
  }
}

// VSCodeのコードコピーのように div/span だけで構成されたHTMLは
// 装飾情報を持たないため、text/plain側のMarkdown推定に回す
function htmlHasRichStructure(html: string) {
  const template = document.createElement("template");
  template.innerHTML = html;
  return Boolean(
    template.content.querySelector(
      "ul,ol,li,h1,h2,h3,h4,h5,h6,table,blockquote,pre,code,strong,b,em,i,s,del,a,img,input[type=checkbox]",
    ),
  );
}

function sanitizePastedHtml(html: string) {
  const template = document.createElement("template");
  template.innerHTML = html;

  normalizeMarkdownTaskLists(template.content);
  preserveEmptyParagraphs(template.content);
  stripUnsafeHtml(template.content);

  return template.innerHTML;
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

  normalizeMarkdownTaskLists(template.content);
  stripUnsafeHtml(template.content);

  return template.innerHTML;
}

function stripUnsafeHtml(root: DocumentFragment) {
  for (const element of Array.from(
    root.querySelectorAll("script,style,iframe,object,embed"),
  )) {
    element.remove();
  }

  for (const element of Array.from(root.querySelectorAll("*"))) {
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
}

function normalizeMarkdownTaskLists(root: ParentNode) {
  for (const list of Array.from(root.querySelectorAll("ul"))) {
    const items = directListItems(list);
    if (!items.some(taskCheckboxFromListItem)) continue;

    list.setAttribute("data-type", "taskList");
    for (const item of items) {
      const checkbox = taskCheckboxFromListItem(item);
      item.setAttribute("data-type", "taskItem");
      item.setAttribute("data-checked", checkbox?.checked ? "true" : "false");

      if (checkbox) {
        const parent = checkbox.parentElement;
        checkbox.remove();
        trimLeadingWhitespace(parent ?? item);
      }
    }
  }
}

function directListItems(list: Element) {
  return Array.from(list.children).filter(
    (child): child is HTMLLIElement => child instanceof HTMLLIElement,
  );
}

function taskCheckboxFromListItem(item: HTMLLIElement) {
  const firstElement = item.firstElementChild;
  if (firstElement instanceof HTMLInputElement && firstElement.type === "checkbox") {
    return firstElement;
  }

  if (firstElement instanceof HTMLParagraphElement) {
    const paragraphFirst = firstElement.firstElementChild;
    if (
      paragraphFirst instanceof HTMLInputElement &&
      paragraphFirst.type === "checkbox"
    ) {
      return paragraphFirst;
    }
  }

  return null;
}

function trimLeadingWhitespace(element: Element) {
  const first = element.firstChild;
  if (!first || first.nodeType !== Node.TEXT_NODE) return;

  first.textContent = first.textContent?.replace(/^\s+/, "") ?? "";
  if (!first.textContent) {
    first.remove();
  }
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
