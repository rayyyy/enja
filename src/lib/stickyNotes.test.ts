import { expect, test } from "bun:test";
import {
  deriveNoteTitle,
  noteToMarkdown,
  serializeRichTextNode,
  type RichTextNode,
} from "./stickyNotes";

test("noteToMarkdown keeps copied blocks compact", () => {
  const doc: RichTextNode = {
    type: "doc",
    content: [
      {
        type: "heading",
        attrs: { level: 1 },
        content: [{ type: "text", text: "disol" }],
      },
      {
        type: "bulletList",
        content: [
          {
            type: "listItem",
            content: [
              {
                type: "paragraph",
                content: [{ type: "text", text: "実装" }],
              },
            ],
          },
        ],
      },
      {
        type: "paragraph",
      },
      {
        type: "paragraph",
        content: [{ type: "text", text: "名古屋商工会議所 -> PR 5/1日から" }],
      },
      {
        type: "bulletList",
        content: [
          {
            type: "listItem",
            content: [
              {
                type: "paragraph",
                content: [{ type: "text", text: "tiktok shop" }],
              },
            ],
          },
          {
            type: "listItem",
            content: [
              {
                type: "paragraph",
                content: [{ type: "text", text: "住民税の納付" }],
              },
            ],
          },
        ],
      },
    ],
  };

  expect(noteToMarkdown(doc)).toBe(
    "# disol\n- 実装\n\n名古屋商工会議所 -> PR 5/1日から\n- tiktok shop\n- 住民税の納付",
  );
});

test("noteToMarkdown renders task list checkboxes", () => {
  const doc: RichTextNode = {
    type: "doc",
    content: [
      {
        type: "taskList",
        content: [
          {
            type: "taskItem",
            attrs: { checked: false },
            content: [
              {
                type: "paragraph",
                content: [{ type: "text", text: "買い物" }],
              },
            ],
          },
          {
            type: "taskItem",
            attrs: { checked: true },
            content: [
              {
                type: "paragraph",
                content: [{ type: "text", text: "完了" }],
              },
            ],
          },
        ],
      },
    ],
  };

  expect(noteToMarkdown(doc)).toBe("- [ ] 買い物\n- [x] 完了");
});

test("deriveNoteTitle uses the first body line", () => {
  const doc: RichTextNode = {
    type: "doc",
    content: [
      {
        type: "heading",
        attrs: { level: 1 },
        content: [{ type: "text", text: "disol" }],
      },
      {
        type: "paragraph",
        content: [{ type: "text", text: "second line" }],
      },
    ],
  };

  expect(deriveNoteTitle(doc)).toBe("disol");
});

test("serializeRichTextNode returns cached string for the same doc object", () => {
  const doc: RichTextNode = {
    type: "doc",
    content: [
      { type: "paragraph", content: [{ type: "text", text: "キャッシュ" }] },
    ],
  };

  const first = serializeRichTextNode(doc);
  const second = serializeRichTextNode(doc);

  expect(first).toBe(JSON.stringify(doc));
  expect(second).toBe(first);
});

test("serializeRichTextNode falls back to default content for invalid docs", () => {
  expect(serializeRichTextNode(null)).toBe(
    JSON.stringify({ type: "doc", content: [{ type: "paragraph" }] }),
  );
  expect(serializeRichTextNode({ type: "paragraph" })).toBe(
    serializeRichTextNode(undefined),
  );
});
