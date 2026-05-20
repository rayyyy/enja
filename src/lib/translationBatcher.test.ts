import { expect, test } from "bun:test";
import { createTranslationBatcher } from "./translationBatcher";

test("translation batcher flushes appended chunks together", async () => {
  const flushed: string[] = [];
  const batcher = createTranslationBatcher((text) => flushed.push(text), 1);

  batcher.append("he");
  batcher.append("llo");

  expect(flushed).toEqual([]);
  await new Promise((resolve) => setTimeout(resolve, 5));

  expect(flushed).toEqual(["hello"]);
});

test("translation batcher can flush synchronously", () => {
  const flushed: string[] = [];
  const batcher = createTranslationBatcher((text) => flushed.push(text), 1000);

  batcher.append("a");
  batcher.append("b");
  batcher.flush();

  expect(flushed).toEqual(["ab"]);
  batcher.dispose();
});
