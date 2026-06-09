import { useEffect, useRef } from "react";
import type { ReactNode } from "react";
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
import {
  Bold,
  Code,
  Heading1,
  Heading2,
  ImagePlus,
  Italic,
  LinkIcon,
  List,
  ListOrdered,
  Quote,
  Redo2,
  Strikethrough,
  Undo2,
} from "lucide-react";
import { saveStickyNoteImage } from "../../lib/commands";
import {
  normalizeRichTextNode,
  noteToMarkdown,
  type RichTextNode,
} from "../../lib/stickyNotes";

type RichNoteEditorProps = {
  noteId: string;
  content: RichTextNode;
  onChange: (content: RichTextNode) => void;
  compact?: boolean;
};

export function RichNoteEditor({
  noteId,
  content,
  onChange,
  compact = false,
}: RichNoteEditorProps) {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const editorRef = useRef<Editor | null>(null);
  const lastContentRef = useRef(JSON.stringify(normalizeRichTextNode(content)));

  async function insertImageFile(file: File) {
    if (!file.type.startsWith("image/")) return;
    const dataBase64 = await fileToDataUrl(file);
    const saved = await saveStickyNoteImage({
      noteId,
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
          void insertImageFile(file);
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
          void insertImageFile(file);
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
      lastContentRef.current = JSON.stringify(next);
      onChange(next);
    },
  });

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
    if (serialized === lastContentRef.current || editor.isFocused) return;
    lastContentRef.current = serialized;
    editor.commands.setContent(normalized as never);
  }, [content, editor]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <EditorToolbar
        editor={editor}
        compact={compact}
        onPickImage={() => fileInputRef.current?.click()}
      />
      <input
        ref={fileInputRef}
        type="file"
        accept="image/png,image/jpeg,image/gif,image/webp"
        className="hidden"
        onChange={(event) => {
          const file = event.currentTarget.files?.[0];
          event.currentTarget.value = "";
          if (file) void insertImageFile(file);
        }}
      />
      <div className="min-h-0 flex-1 overflow-y-auto">
        <EditorContent editor={editor} />
      </div>
    </div>
  );
}

function EditorToolbar({
  editor,
  compact,
  onPickImage,
}: {
  editor: Editor | null;
  compact: boolean;
  onPickImage: () => void;
}) {
  function setLink() {
    if (!editor) return;
    if (editor.isActive("link")) {
      editor.chain().focus().unsetLink().run();
      return;
    }
    const href = window.prompt("URL");
    if (!href) return;
    editor.chain().focus().setLink({ href }).run();
  }

  return (
    <div
      className={`flex shrink-0 items-center gap-1 border-b border-black/5 px-3 ${
        compact ? "h-9 overflow-x-auto" : "h-10"
      }`}
    >
      <ToolbarButton
        label="元に戻す"
        disabled={!editor}
        onClick={() => editor?.chain().focus().undo().run()}
      >
        <Undo2 size={15} />
      </ToolbarButton>
      <ToolbarButton
        label="やり直す"
        disabled={!editor}
        onClick={() => editor?.chain().focus().redo().run()}
      >
        <Redo2 size={15} />
      </ToolbarButton>
      <Divider />
      <ToolbarButton
        label="太字"
        active={editor?.isActive("bold")}
        disabled={!editor}
        onClick={() => editor?.chain().focus().toggleBold().run()}
      >
        <Bold size={15} />
      </ToolbarButton>
      <ToolbarButton
        label="斜体"
        active={editor?.isActive("italic")}
        disabled={!editor}
        onClick={() => editor?.chain().focus().toggleItalic().run()}
      >
        <Italic size={15} />
      </ToolbarButton>
      <ToolbarButton
        label="取り消し線"
        active={editor?.isActive("strike")}
        disabled={!editor}
        onClick={() => editor?.chain().focus().toggleStrike().run()}
      >
        <Strikethrough size={15} />
      </ToolbarButton>
      <ToolbarButton
        label="コード"
        active={editor?.isActive("code")}
        disabled={!editor}
        onClick={() => editor?.chain().focus().toggleCode().run()}
      >
        <Code size={15} />
      </ToolbarButton>
      <Divider />
      <ToolbarButton
        label="見出し1"
        active={editor?.isActive("heading", { level: 1 })}
        disabled={!editor}
        onClick={() => editor?.chain().focus().toggleHeading({ level: 1 }).run()}
      >
        <Heading1 size={15} />
      </ToolbarButton>
      <ToolbarButton
        label="見出し2"
        active={editor?.isActive("heading", { level: 2 })}
        disabled={!editor}
        onClick={() => editor?.chain().focus().toggleHeading({ level: 2 }).run()}
      >
        <Heading2 size={15} />
      </ToolbarButton>
      <ToolbarButton
        label="箇条書き"
        active={editor?.isActive("bulletList")}
        disabled={!editor}
        onClick={() => editor?.chain().focus().toggleBulletList().run()}
      >
        <List size={15} />
      </ToolbarButton>
      <ToolbarButton
        label="番号付きリスト"
        active={editor?.isActive("orderedList")}
        disabled={!editor}
        onClick={() => editor?.chain().focus().toggleOrderedList().run()}
      >
        <ListOrdered size={15} />
      </ToolbarButton>
      <ToolbarButton
        label="引用"
        active={editor?.isActive("blockquote")}
        disabled={!editor}
        onClick={() => editor?.chain().focus().toggleBlockquote().run()}
      >
        <Quote size={15} />
      </ToolbarButton>
      <Divider />
      <ToolbarButton label="リンク" disabled={!editor} onClick={setLink}>
        <LinkIcon size={15} />
      </ToolbarButton>
      <ToolbarButton label="画像" disabled={!editor} onClick={onPickImage}>
        <ImagePlus size={15} />
      </ToolbarButton>
    </div>
  );
}

function ToolbarButton({
  label,
  active = false,
  disabled = false,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className={`grid size-7 shrink-0 place-items-center rounded-md transition-colors disabled:pointer-events-none disabled:opacity-40 ${
        active
          ? "bg-neutral-900 text-white"
          : "text-neutral-500 hover:bg-black/5 hover:text-neutral-800"
      }`}
    >
      {children}
    </button>
  );
}

function Divider() {
  return <span className="mx-1 h-4 w-px shrink-0 bg-black/10" />;
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
  const normalizedBullets = markdown.replace(
    /^(\s*)[・•●]\s*/gm,
    "$1- ",
  );
  const lines = normalizedBullets.split(/\r?\n/);
  const out: string[] = [];
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const previous = out[out.length - 1] ?? "";
    if (
      line.trim() &&
      isListLine(previous) &&
      !isListLine(line) &&
      !isIndentedLine(line) &&
      !isMarkdownBlockBoundary(line)
    ) {
      out.push("");
    }
    out.push(line);
  }
  return out.join("\n");
}

function isListLine(line: string) {
  return /^\s{0,3}(?:[-*+]\s+\S|\d+[.)]\s+\S)/.test(line);
}

function isIndentedLine(line: string) {
  return /^(?: {2,}|\t)/.test(line);
}

function isMarkdownBlockBoundary(line: string) {
  return /^\s{0,3}(?:#{1,6}\s+|>\s+|```|---\s*$)/.test(line);
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
