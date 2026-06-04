export type LinkifiedTextPart =
  | {
      type: "text";
      text: string;
    }
  | {
      type: "url";
      text: string;
      href: string;
    };

const URL_PATTERN =
  /(^|[^\p{L}\p{N}_])((?:https?:\/\/|www\.)[^\s<>"']+)/giu;

const TRAILING_PUNCTUATION = new Set([
  ".",
  ",",
  "!",
  "?",
  ";",
  ":",
  "、",
  "。",
  "！",
  "？",
  "；",
  "：",
]);

const CLOSING_BRACKETS = new Map([
  [")", "("],
  ["]", "["],
  ["}", "{"],
  ["）", "（"],
  ["］", "［"],
  ["｝", "｛"],
]);

function countCharacter(text: string, character: string) {
  let count = 0;

  for (const current of text) {
    if (current === character) count += 1;
  }

  return count;
}

function splitTrailingText(urlText: string) {
  let end = urlText.length;

  while (end > 0) {
    const lastCharacter = urlText[end - 1];
    if (!lastCharacter) break;

    if (TRAILING_PUNCTUATION.has(lastCharacter)) {
      end -= lastCharacter.length;
      continue;
    }

    const openingBracket = CLOSING_BRACKETS.get(lastCharacter);
    if (openingBracket) {
      const candidate = urlText.slice(0, end);
      if (
        countCharacter(candidate, lastCharacter) >
        countCharacter(candidate, openingBracket)
      ) {
        end -= lastCharacter.length;
        continue;
      }
    }

    break;
  }

  return {
    linkText: urlText.slice(0, end),
    trailingText: urlText.slice(end),
  };
}

function toHref(urlText: string) {
  return /^www\./i.test(urlText) ? `https://${urlText}` : urlText;
}

function isSupportedUrl(urlText: string) {
  try {
    const url = new URL(toHref(urlText));
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

export function linkifyUrlText(text: string): LinkifiedTextPart[] {
  const parts: LinkifiedTextPart[] = [];
  let lastIndex = 0;

  for (const match of text.matchAll(URL_PATTERN)) {
    const prefix = match[1] ?? "";
    const rawUrlText = match[2] ?? "";
    const urlStart = match.index + prefix.length;
    const urlEnd = urlStart + rawUrlText.length;
    const { linkText, trailingText } = splitTrailingText(rawUrlText);

    if (!linkText || !isSupportedUrl(linkText)) continue;

    if (lastIndex < urlStart) {
      parts.push({ type: "text", text: text.slice(lastIndex, urlStart) });
    }

    parts.push({
      type: "url",
      text: linkText,
      href: toHref(linkText),
    });

    if (trailingText) {
      parts.push({ type: "text", text: trailingText });
    }

    lastIndex = urlEnd;
  }

  if (lastIndex < text.length) {
    parts.push({ type: "text", text: text.slice(lastIndex) });
  }

  return parts.length > 0 ? parts : [{ type: "text", text }];
}
