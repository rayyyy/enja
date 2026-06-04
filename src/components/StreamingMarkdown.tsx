import { openUrl } from "@tauri-apps/plugin-opener";
import type { MouseEvent } from "react";
import { linkifyUrlText } from "../lib/linkifyText";

interface Props {
  text: string;
  streaming: boolean;
}

function handleLinkClick(event: MouseEvent<HTMLAnchorElement>, href: string) {
  event.preventDefault();
  void openUrl(href);
}

/** 翻訳本文のみをプレーンテキストで表示（見出し・Markdownは使わない） */
export function StreamingMarkdown({ text, streaming }: Props) {
  const parts = linkifyUrlText(text);

  return (
    <div className="text-[13px] leading-relaxed text-neutral-700">
      {text ? (
        <p className="m-0 whitespace-pre-wrap wrap-break-word">
          {parts.map((part, index) =>
            part.type === "url" ? (
              <a
                key={`${part.href}-${index}`}
                href={part.href}
                onClick={(event) => handleLinkClick(event, part.href)}
                className="text-blue-600 underline decoration-blue-300 underline-offset-2 transition-colors hover:text-blue-700"
                rel="noreferrer"
                target="_blank"
              >
                {part.text}
              </a>
            ) : (
              part.text
            ),
          )}
        </p>
      ) : (
        <p className="m-0 text-neutral-400">
          {streaming ? "応答を待っています…" : "翻訳がここに表示されます。"}
        </p>
      )}
      {streaming ? (
        <span
          className="ml-0.5 inline-block h-4 w-0.5 animate-pulse bg-blue-500 align-middle"
          aria-hidden
        />
      ) : null}
    </div>
  );
}
