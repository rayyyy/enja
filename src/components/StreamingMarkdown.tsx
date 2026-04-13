interface Props {
  text: string;
  streaming: boolean;
}

/** 翻訳本文のみをプレーンテキストで表示（見出し・Markdownは使わない） */
export function StreamingMarkdown({ text, streaming }: Props) {
  return (
    <div className="text-[13px] leading-relaxed text-neutral-700">
      {text ? (
        <p className="m-0 whitespace-pre-wrap wrap-break-word">{text}</p>
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
