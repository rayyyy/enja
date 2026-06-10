import { openUrl } from "@tauri-apps/plugin-opener";
import { useMemo, type MouseEvent } from "react";
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
  const parts = useMemo(() => linkifyUrlText(text), [text]);

  return (
    <div className="text-[14px] leading-relaxed text-ink">
      {text ? (
        <p className="m-0 whitespace-pre-wrap wrap-break-word">
          {parts.map((part, index) =>
            part.type === "url" ? (
              <a
                key={`${part.href}-${index}`}
                href={part.href}
                onClick={(event) => handleLinkClick(event, part.href)}
                className="text-accent-ink underline decoration-accent/40 underline-offset-2 transition-colors hover:decoration-accent"
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
        <p className="m-0 text-ink-faint">
          {streaming ? "応答を待っています…" : "翻訳がここに表示されます。"}
        </p>
      )}
      {streaming ? (
        <span
          className="ml-0.5 inline-block h-4 w-0.5 animate-pulse rounded-full bg-accent align-middle"
          aria-hidden
        />
      ) : null}
    </div>
  );
}
