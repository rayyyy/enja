import type { StickyNoteColor } from "../types";

export type RichTextNode = {
  type?: string;
  text?: string;
  attrs?: Record<string, unknown>;
  marks?: Array<{ type?: string; attrs?: Record<string, unknown> }>;
  content?: RichTextNode[];
};

export const defaultNoteContent: RichTextNode = {
  type: "doc",
  content: [{ type: "paragraph" }],
};

// 実色は index.css の --note-*-surface / --note-*-swatch（ライト/ダークで切替）。
export const noteColorPresets: Array<{
  id: StickyNoteColor;
  label: string;
  swatch: string;
}> = [
  { id: "lemon", label: "レモン", swatch: "var(--note-lemon-swatch)" },
  { id: "mint", label: "ミント", swatch: "var(--note-mint-swatch)" },
  { id: "sky", label: "スカイ", swatch: "var(--note-sky-swatch)" },
  { id: "rose", label: "ローズ", swatch: "var(--note-rose-swatch)" },
  { id: "paper", label: "ペーパー", swatch: "var(--note-paper-swatch)" },
];

export function noteColorClass(color: StickyNoteColor, surface = false) {
  const classes: Record<StickyNoteColor, string> = {
    lemon: surface
      ? "bg-[var(--note-lemon-surface)]"
      : "bg-[var(--note-lemon-swatch)]",
    mint: surface
      ? "bg-[var(--note-mint-surface)]"
      : "bg-[var(--note-mint-swatch)]",
    sky: surface ? "bg-[var(--note-sky-surface)]" : "bg-[var(--note-sky-swatch)]",
    rose: surface
      ? "bg-[var(--note-rose-surface)]"
      : "bg-[var(--note-rose-swatch)]",
    paper: surface
      ? "bg-[var(--note-paper-surface)]"
      : "bg-[var(--note-paper-swatch)]",
  };
  return classes[color] ?? classes.lemon;
}

export function normalizeRichTextNode(value: unknown): RichTextNode {
  if (
    value &&
    typeof value === "object" &&
    "type" in value &&
    (value as RichTextNode).type === "doc"
  ) {
    return value as RichTextNode;
  }
  return defaultNoteContent;
}

export function extractPlainText(doc: unknown): string {
  const node = normalizeRichTextNode(doc);
  return collectText(node).replace(/\s+/g, " ").trim();
}

export function deriveNoteTitle(doc: unknown): string {
  const node = normalizeRichTextNode(doc);
  const title = firstTextLine(node).replace(/\s+/g, " ").trim();
  return title ? title.slice(0, 80) : "無題のメモ";
}

export function noteToMarkdown(doc: unknown): string {
  const node = normalizeRichTextNode(doc);
  return renderBlocks(node.content ?? []).trim();
}

function collectText(node: RichTextNode): string {
  if (node.text) return node.text;
  if (node.type === "image") return " 画像 ";
  return (node.content ?? []).map(collectText).join(" ");
}

function firstTextLine(node: RichTextNode): string {
  if (node.text) return node.text;
  if (node.type === "image") return "画像";
  for (const child of node.content ?? []) {
    const value = firstTextLine(child);
    if (value.trim()) return value;
  }
  return "";
}

function renderBlocks(nodes: RichTextNode[], depth = 0): string {
  const rendered = trimBlankEdges(
    nodes
    .map((node, index) => ({
      node,
      value: renderBlock(node, depth, index),
      blank: isBlankBlock(node),
    })),
  );

  let output = "";
  let previous: RichTextNode | null = null;
  let pendingBlankBlocks = 0;

  for (const block of rendered) {
    if (block.blank) {
      pendingBlankBlocks += 1;
      continue;
    }

    if (!output) {
      output = block.value;
    } else if (previous) {
      output += `${blockSeparator()}${"\n".repeat(pendingBlankBlocks)}${block.value}`;
    }
    previous = block.node;
    pendingBlankBlocks = 0;
  }

  return output;
}

function renderBlock(node: RichTextNode, depth: number, index: number): string {
  switch (node.type) {
    case "paragraph":
      return renderInline(node.content ?? []);
    case "heading": {
      const level = Number(node.attrs?.level ?? 1);
      return `${"#".repeat(Math.min(Math.max(level, 1), 6))} ${renderInline(
        node.content ?? [],
      )}`;
    }
    case "bulletList":
      return renderList(node.content ?? [], depth, false);
    case "orderedList":
      return renderList(node.content ?? [], depth, true);
    case "listItem":
      return renderListItem(node, depth, false, index);
    case "taskList":
      return renderTaskList(node.content ?? [], depth);
    case "taskItem":
      return renderTaskItem(node, depth, index);
    case "blockquote":
      return renderBlocks(node.content ?? [], depth)
        .split("\n")
        .map((line) => `> ${line}`)
        .join("\n");
    case "codeBlock": {
      const language = String(node.attrs?.language ?? "");
      return `\`\`\`${language}\n${collectText(node).trimEnd()}\n\`\`\``;
    }
    case "horizontalRule":
      return "---";
    case "image":
      return renderImage(node);
    default:
      return renderBlocks(node.content ?? [], depth);
  }
}

function renderInline(nodes: RichTextNode[]): string {
  return nodes.map(renderInlineNode).join("");
}

function renderInlineNode(node: RichTextNode): string {
  if (node.type === "hardBreak") return "\n";
  if (node.type === "image") return renderImage(node);

  let value = node.text ?? renderInline(node.content ?? []);
  for (const mark of node.marks ?? []) {
    switch (mark.type) {
      case "bold":
        value = `**${value}**`;
        break;
      case "italic":
        value = `*${value}*`;
        break;
      case "strike":
        value = `~~${value}~~`;
        break;
      case "code":
        value = `\`${value.replace(/`/g, "\\`")}\``;
        break;
      case "link": {
        const href = String(mark.attrs?.href ?? "");
        if (href) value = `[${value}](${href})`;
        break;
      }
    }
  }
  return value;
}

function renderList(nodes: RichTextNode[], depth: number, ordered: boolean) {
  return nodes
    .map((node, index) => renderListItem(node, depth, ordered, index))
    .join("\n");
}

function renderListItem(
  node: RichTextNode,
  depth: number,
  ordered: boolean,
  index: number,
) {
  const indent = "  ".repeat(depth);
  const marker = ordered ? `${index + 1}.` : "-";
  const children = node.content ?? [];
  const first = children[0];
  const firstLine =
    first?.type === "paragraph"
      ? renderInline(first.content ?? [])
      : renderBlock(first ?? { type: "paragraph" }, depth + 1, index);
  const rest = children
    .slice(1)
    .map((child, childIndex) => renderBlock(child, depth + 1, childIndex))
    .filter(Boolean)
    .map((line) =>
      line
        .split("\n")
        .map((part) => `${indent}  ${part}`)
        .join("\n"),
    );
  return [`${indent}${marker} ${firstLine}`, ...rest].join("\n");
}

function renderTaskList(nodes: RichTextNode[], depth: number) {
  return nodes.map((node, index) => renderTaskItem(node, depth, index)).join("\n");
}

function renderTaskItem(node: RichTextNode, depth: number, index: number) {
  const indent = "  ".repeat(depth);
  const marker = node.attrs?.checked ? "- [x]" : "- [ ]";
  const children = node.content ?? [];
  const first = children[0];
  const firstLine =
    first?.type === "paragraph"
      ? renderInline(first.content ?? [])
      : renderBlock(first ?? { type: "paragraph" }, depth + 1, index);
  const rest = children
    .slice(1)
    .map((child, childIndex) => renderBlock(child, depth + 1, childIndex))
    .filter(Boolean)
    .map((line) =>
      line
        .split("\n")
        .map((part) => `${indent}  ${part}`)
        .join("\n"),
    );
  return [`${indent}${marker} ${firstLine}`, ...rest].join("\n");
}

function renderImage(node: RichTextNode): string {
  const alt = String(node.attrs?.alt ?? "image");
  const src = String(node.attrs?.title || node.attrs?.src || "");
  return src ? `![${alt}](${src})` : "";
}

function blockSeparator() {
  return "\n";
}

function isBlankBlock(node: RichTextNode) {
  if (node.type === "paragraph") {
    return renderInline(node.content ?? []).trim().length === 0;
  }
  return false;
}

function trimBlankEdges<T extends { blank: boolean }>(blocks: T[]) {
  let start = 0;
  let end = blocks.length;
  while (start < end && blocks[start].blank) start += 1;
  while (end > start && blocks[end - 1].blank) end -= 1;
  return blocks.slice(start, end);
}
