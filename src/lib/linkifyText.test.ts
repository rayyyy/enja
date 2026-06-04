import { expect, test } from "bun:test";
import { linkifyUrlText } from "./linkifyText";

test("linkifies http and https urls while preserving surrounding text", () => {
  expect(linkifyUrlText("See https://example.com/path?q=1 now")).toEqual([
    { type: "text", text: "See " },
    {
      type: "url",
      text: "https://example.com/path?q=1",
      href: "https://example.com/path?q=1",
    },
    { type: "text", text: " now" },
  ]);
});

test("normalizes www urls to https hrefs", () => {
  expect(linkifyUrlText("Open www.example.com/docs")).toEqual([
    { type: "text", text: "Open " },
    {
      type: "url",
      text: "www.example.com/docs",
      href: "https://www.example.com/docs",
    },
  ]);
});

test("keeps sentence punctuation outside urls", () => {
  expect(linkifyUrlText("詳しくは https://example.com/docs）。")).toEqual([
    { type: "text", text: "詳しくは " },
    {
      type: "url",
      text: "https://example.com/docs",
      href: "https://example.com/docs",
    },
    { type: "text", text: "）。" },
  ]);
});

test("keeps balanced parentheses inside urls", () => {
  expect(linkifyUrlText("Ref https://example.com/path_(demo).")).toEqual([
    { type: "text", text: "Ref " },
    {
      type: "url",
      text: "https://example.com/path_(demo)",
      href: "https://example.com/path_(demo)",
    },
    { type: "text", text: "." },
  ]);
});
