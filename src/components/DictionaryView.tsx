import { useEffect, useMemo, useState } from "react";
import type { DictionaryCorrection, DictionaryEntry } from "../types";
import {
  createDictionaryEntries,
  deleteDictionaryEntry,
  getDictionary,
  updateDictionaryEntry,
} from "../lib/commands";

// バックエンド validate_entry の上限（dictionary.rs）と一致させる。
const MAX_WORD_LENGTH = 100;

type EditingEntry = {
  id: string;
  preferred: string;
  aliasesText: string;
  correctionsText: string;
  enabled: boolean;
};

export function DictionaryView() {
  const [entries, setEntries] = useState<DictionaryEntry[]>([]);
  const [draft, setDraft] = useState("");
  const [editing, setEditing] = useState<EditingEntry | null>(null);
  const [query, setQuery] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    void refresh();
  }, []);

  async function refresh() {
    setEntries(await getDictionary());
  }

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return entries;
    return entries.filter((entry) =>
      [
        entry.preferred,
        ...entry.aliases,
        ...entry.corrections.flatMap((correction) => [
          correction.from,
          correction.to,
        ]),
      ].some((value) => value.toLowerCase().includes(q)),
    );
  }, [entries, query]);

  async function addWords() {
    const words = draft
      .split("\n")
      .map((w) => w.trim())
      .filter(Boolean);
    if (words.length === 0) return;
    // 文字数超過はバックエンドでも弾かれるが、理由を区別して伝えるため事前に分ける。
    const tooLong = words.filter((w) => [...w].length > MAX_WORD_LENGTH);
    const valid = words.filter((w) => [...w].length <= MAX_WORD_LENGTH);
    setSaving(true);
    setMessage(null);
    try {
      const result = valid.length
        ? await createDictionaryEntries(
            valid.map((preferred) => ({ preferred, enabled: true })),
          )
        : { added: [], skipped: 0 };
      const reasons: string[] = [];
      if (result.skipped) reasons.push(`重複${result.skipped}件`);
      if (tooLong.length) reasons.push(`文字数超過${tooLong.length}件`);
      const suffix = reasons.length ? `（${reasons.join("・")}はスキップ）` : "";
      setMessage(`${result.added.length}件追加しました${suffix}`);
      // 追加済み・重複分は入力欄から消し、修正が必要な文字数超過行だけ残す。
      setDraft(tooLong.join("\n"));
      await refresh();
    } catch (e) {
      setMessage(String(e));
    } finally {
      setSaving(false);
    }
  }

  async function toggleEntry(entry: DictionaryEntry) {
    setMessage(null);
    try {
      await updateDictionaryEntry(entry.id, {
        preferred: entry.preferred,
        enabled: !entry.enabled,
      });
      await refresh();
    } catch (e) {
      setMessage(String(e));
    }
  }

  function startEditing(entry: DictionaryEntry) {
    setMessage(null);
    setEditing({
      id: entry.id,
      preferred: entry.preferred,
      aliasesText: entry.aliases.join("\n"),
      correctionsText: formatCorrections(entry.corrections),
      enabled: entry.enabled,
    });
  }

  async function saveEditing() {
    if (!editing) return;
    const preferred = editing.preferred.trim();
    const aliases = parseList(editing.aliasesText);
    const parsedCorrections = parseCorrections(editing.correctionsText);
    const tooLong =
      [...preferred].length > MAX_WORD_LENGTH ||
      aliases.some((alias) => [...alias].length > MAX_WORD_LENGTH) ||
      parsedCorrections.corrections.some(
        (correction) =>
          [...correction.from].length > MAX_WORD_LENGTH ||
          [...correction.to].length > MAX_WORD_LENGTH,
      );

    if (!preferred) {
      setMessage("単語を入力してください。");
      return;
    }
    if (tooLong) {
      setMessage("単語・誤認識した表記・補正ルールは100文字以内にしてください。");
      return;
    }
    if (parsedCorrections.invalid.length) {
      setMessage("補正ルールは「誤認識 -> 正しい表記」の形で入力してください。");
      return;
    }

    setSaving(true);
    setMessage(null);
    try {
      await updateDictionaryEntry(editing.id, {
        preferred,
        aliases,
        corrections: parsedCorrections.corrections,
        enabled: editing.enabled,
      });
      setEditing(null);
      setMessage("更新しました。");
      await refresh();
    } catch (e) {
      setMessage(String(e));
    } finally {
      setSaving(false);
    }
  }

  async function removeEntry(id: string) {
    setMessage(null);
    try {
      await deleteDictionaryEntry(id);
      if (editing?.id === id) {
        setEditing(null);
      }
      await refresh();
    } catch (e) {
      setMessage(String(e));
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-5">
      <header>
        <div>
          <h1 className="text-xl font-semibold tracking-tight text-ink">辞書</h1>
          <p className="mt-1 text-sm leading-relaxed text-ink-mid">
            音声認識と整形で優先する単語を登録します。各モデルに自動で連携されます。
          </p>
        </div>
      </header>

      <section className="flex flex-col gap-2 rounded-xl border border-edge bg-sunken p-4">
        <label className="flex flex-col gap-1 text-sm">
          <span className="font-medium text-ink">単語を追加</span>
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
                e.preventDefault();
                void addWords();
              }
            }}
            rows={4}
            placeholder={"1行に1単語（改行で一括登録）\n例:\nTypeless\nAquaVoice"}
            className="resize-none rounded-md border border-edge bg-surface px-3 py-2 text-sm text-ink outline-none transition-[border-color,box-shadow] duration-100 placeholder:text-ink-faint focus:border-accent focus:ring-2 focus:ring-accent/25"
          />
        </label>
        <div className="flex items-center gap-3">
          <button
            type="button"
            disabled={saving || !draft.trim()}
            onClick={() => void addWords()}
            className="rounded-md bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-100 focus-ring hover:bg-accent-deep active:scale-[0.98] disabled:opacity-40"
          >
            {saving ? "追加中…" : "追加"}
          </button>
          {message ? <p className="text-sm text-ink-mid">{message}</p> : null}
        </div>
      </section>

      <div className="flex items-center justify-between gap-3">
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="辞書を検索"
          className="w-full max-w-xs text-sm rounded-md border border-edge bg-surface px-3 py-2 text-sm text-ink outline-none transition-[border-color,box-shadow] duration-100 placeholder:text-ink-faint focus:border-accent focus:ring-2 focus:ring-accent/25"
        />
        <p className="text-xs text-ink-faint">{entries.length}件</p>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto rounded-xl border border-edge bg-surface">
        {filtered.length === 0 ? (
          <p className="p-5 text-sm text-ink-faint">登録された単語はありません。</p>
        ) : (
          <ul className="divide-y divide-edge">
            {filtered.map((entry) => {
              const details = entryDetails(entry);
              const isEditing = editing?.id === entry.id;
              return (
                <li key={entry.id} className="px-3 py-2.5">
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={() => void toggleEntry(entry)}
                      disabled={saving}
                      title={entry.enabled ? "有効（クリックで無効化）" : "無効（クリックで有効化）"}
                      className="flex size-6 shrink-0 items-center justify-center rounded-full transition-colors duration-100 hover:bg-hover disabled:opacity-50"
                      aria-label={entry.enabled ? "無効化" : "有効化"}
                    >
                      <span
                        className={`size-2.5 rounded-full ${entry.enabled ? "bg-accent" : "bg-edge-strong"}`}
                        aria-hidden
                      />
                    </button>
                    <div className="min-w-0 flex-1">
                      <p
                        className={`truncate text-sm ${entry.enabled ? "text-ink" : "text-ink-faint"}`}
                      >
                        {entry.preferred}
                      </p>
                      {details ? (
                        <p className="mt-0.5 truncate text-xs text-ink-faint">{details}</p>
                      ) : null}
                    </div>
                    <button
                      type="button"
                      onClick={() => (isEditing ? setEditing(null) : startEditing(entry))}
                      disabled={saving}
                      className="rounded-md border border-edge bg-surface px-2.5 py-1 text-xs text-ink-mid transition-colors duration-100 hover:bg-hover hover:text-ink disabled:opacity-50"
                    >
                      {isEditing ? "閉じる" : "詳細"}
                    </button>
                    <button
                      type="button"
                      onClick={() => void removeEntry(entry.id)}
                      disabled={saving}
                      className="rounded-md px-2.5 py-1 text-xs text-danger transition-colors duration-100 hover:bg-danger-soft disabled:opacity-50"
                    >
                      削除
                    </button>
                  </div>
                  {isEditing && editing ? (
                    <div className="mt-3 grid gap-3 border-t border-edge pt-3">
                      <label className="flex flex-col gap-1 text-sm">
                        <span className="font-medium text-ink">優先表記</span>
                        <input
                          value={editing.preferred}
                          onChange={(e) =>
                            setEditing({ ...editing, preferred: e.target.value })
                          }
                          className="rounded-md border border-edge bg-surface px-3 py-2 text-sm text-ink outline-none transition-[border-color,box-shadow] duration-100 placeholder:text-ink-faint focus:border-accent focus:ring-2 focus:ring-accent/25"
                        />
                      </label>
                      <label className="flex flex-col gap-1 text-sm">
                        <span className="font-medium text-ink">誤認識した表記</span>
                        <textarea
                          value={editing.aliasesText}
                          onChange={(e) =>
                            setEditing({ ...editing, aliasesText: e.target.value })
                          }
                          rows={3}
                          placeholder={"1行に1つ\n例:\nタイプレス\nタイプです"}
                          className="resize-none rounded-md border border-edge bg-surface px-3 py-2 text-sm text-ink outline-none transition-[border-color,box-shadow] duration-100 placeholder:text-ink-faint focus:border-accent focus:ring-2 focus:ring-accent/25"
                        />
                      </label>
                      <label className="flex flex-col gap-1 text-sm">
                        <span className="font-medium text-ink">高度な補正</span>
                        <textarea
                          value={editing.correctionsText}
                          onChange={(e) =>
                            setEditing({ ...editing, correctionsText: e.target.value })
                          }
                          rows={3}
                          placeholder={"1行に1つ\n例:\nタイプですか？アクアボイス -> TypelessかAquaVoice"}
                          className="resize-none rounded-md border border-edge bg-surface px-3 py-2 text-sm text-ink outline-none transition-[border-color,box-shadow] duration-100 placeholder:text-ink-faint focus:border-accent focus:ring-2 focus:ring-accent/25"
                        />
                      </label>
                      <div className="flex items-center gap-2">
                        <label className="flex items-center gap-2 text-sm text-ink">
                          <input
                            type="checkbox"
                            checked={editing.enabled}
                            onChange={(e) =>
                              setEditing({ ...editing, enabled: e.target.checked })
                            }
                            className="size-4 rounded accent-[var(--accent)]"
                          />
                          有効
                        </label>
                        <button
                          type="button"
                          onClick={() => void saveEditing()}
                          disabled={saving}
                          className="ml-auto rounded-md bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-100 focus-ring hover:bg-accent-deep active:scale-[0.98] disabled:opacity-40"
                        >
                          {saving ? "保存中…" : "保存"}
                        </button>
                      </div>
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}

function parseList(value: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of value.split(/[,\n]/)) {
    const item = raw.trim();
    if (!item || seen.has(item)) continue;
    seen.add(item);
    out.push(item);
  }
  return out;
}

function parseCorrections(value: string): {
  corrections: DictionaryCorrection[];
  invalid: string[];
} {
  const corrections: DictionaryCorrection[] = [];
  const invalid: string[] = [];
  const seen = new Set<string>();
  for (const raw of value.split("\n")) {
    const line = raw.trim();
    if (!line) continue;
    const match = line.match(/^(.*?)\s*(?:->|=>|→)\s*(.*?)$/);
    if (!match) {
      invalid.push(line);
      continue;
    }
    const from = match[1].trim();
    const to = match[2].trim();
    if (!from || !to) {
      invalid.push(line);
      continue;
    }
    if (from === to || seen.has(from)) continue;
    seen.add(from);
    corrections.push({ from, to });
  }
  return { corrections, invalid };
}

function formatCorrections(corrections: DictionaryCorrection[]): string {
  return corrections
    .map((correction) => `${correction.from} -> ${correction.to}`)
    .join("\n");
}

function entryDetails(entry: DictionaryEntry): string {
  const parts: string[] = [];
  if (entry.aliases.length) {
    parts.push(`誤認識: ${entry.aliases.join(" / ")}`);
  }
  if (entry.corrections.length) {
    parts.push(`補正${entry.corrections.length}件`);
  }
  return parts.join(" ・ ");
}
