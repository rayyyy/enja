import { useEffect, useMemo, useState } from "react";
import type { DictionaryEntry, DictionaryEntryInput } from "../types";
import {
  createDictionaryEntry,
  deleteDictionaryEntry,
  getDictionary,
  updateDictionaryEntry,
} from "../lib/commands";
import { useAppStore } from "../stores/useAppStore";

const EMPTY_INPUT: DictionaryEntryInput = {
  preferred: "",
  readings: [],
  aliases: [],
  enabled: true,
};

export function DictionaryView() {
  const setView = useAppStore((s) => s.setView);
  const [entries, setEntries] = useState<DictionaryEntry[]>([]);
  const [input, setInput] = useState<DictionaryEntryInput>(EMPTY_INPUT);
  const [editingId, setEditingId] = useState<string | null>(null);
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
      [entry.preferred, ...entry.readings, ...entry.aliases].some((value) =>
        value.toLowerCase().includes(q),
      ),
    );
  }, [entries, query]);

  function setListField(field: "readings" | "aliases", value: string) {
    setInput((prev) => ({
      ...prev,
      [field]: value
        .split(/[,\n]/)
        .map((v) => v.trim())
        .filter(Boolean),
    }));
  }

  async function saveEntry() {
    setSaving(true);
    setMessage(null);
    try {
      if (editingId) {
        await updateDictionaryEntry(editingId, input);
        setMessage("更新しました。");
      } else {
        await createDictionaryEntry(input);
        setMessage("追加しました。");
      }
      setInput(EMPTY_INPUT);
      setEditingId(null);
      await refresh();
    } catch (e) {
      setMessage(String(e));
    } finally {
      setSaving(false);
    }
  }

  function editEntry(entry: DictionaryEntry) {
    setEditingId(entry.id);
    setInput({
      preferred: entry.preferred,
      readings: entry.readings,
      aliases: entry.aliases,
      enabled: entry.enabled,
    });
  }

  async function removeEntry(id: string) {
    setMessage(null);
    try {
      await deleteDictionaryEntry(id);
      if (editingId === id) {
        setEditingId(null);
        setInput(EMPTY_INPUT);
      }
      await refresh();
    } catch (e) {
      setMessage(String(e));
    }
  }

  return (
    <div className="flex max-h-[560px] min-h-0 flex-col gap-5">
      <header className="flex items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold text-neutral-900">辞書</h1>
          <p className="mt-1 text-sm text-neutral-500">
            音声認識と整形で優先する表記を登録します。
          </p>
        </div>
        <button
          type="button"
          onClick={() => setView("translation")}
          className="rounded-md border border-neutral-200 px-3 py-1.5 text-sm text-neutral-600 hover:bg-neutral-50"
        >
          戻る
        </button>
      </header>

      <section className="grid gap-3 rounded-lg border border-neutral-200 bg-white p-4 md:grid-cols-[1.2fr_1fr]">
        <label className="flex flex-col gap-1 text-sm">
          <span className="font-medium text-neutral-700">優先表記</span>
          <input
            value={input.preferred}
            onChange={(e) => setInput((prev) => ({ ...prev, preferred: e.target.value }))}
            placeholder="例: 岩佐"
            className="rounded-md border border-neutral-200 px-3 py-2 outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20"
          />
        </label>
        <label className="flex items-end gap-2 text-sm text-neutral-700">
          <input
            type="checkbox"
            checked={input.enabled}
            onChange={(e) => setInput((prev) => ({ ...prev, enabled: e.target.checked }))}
            className="mb-2 size-4 rounded border-neutral-300"
          />
          <span className="mb-1.5">有効</span>
        </label>
        <label className="flex flex-col gap-1 text-sm">
          <span className="font-medium text-neutral-700">読み・候補</span>
          <input
            value={input.readings.join(", ")}
            onChange={(e) => setListField("readings", e.target.value)}
            placeholder="例: いわさ, イワサ"
            className="rounded-md border border-neutral-200 px-3 py-2 outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20"
          />
        </label>
        <label className="flex flex-col gap-1 text-sm">
          <span className="font-medium text-neutral-700">別名・誤変換候補</span>
          <input
            value={input.aliases.join(", ")}
            onChange={(e) => setListField("aliases", e.target.value)}
            placeholder="例: iwasa"
            className="rounded-md border border-neutral-200 px-3 py-2 outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20"
          />
        </label>
        <div className="flex items-center gap-2 md:col-span-2">
          <button
            type="button"
            disabled={saving || !input.preferred.trim()}
            onClick={() => void saveEntry()}
            className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-40"
          >
            {saving ? "保存中…" : editingId ? "更新" : "追加"}
          </button>
          {editingId ? (
            <button
              type="button"
              onClick={() => {
                setEditingId(null);
                setInput(EMPTY_INPUT);
              }}
              className="rounded-md border border-neutral-200 px-4 py-2 text-sm text-neutral-600 hover:bg-neutral-50"
            >
              キャンセル
            </button>
          ) : null}
          {message ? <p className="text-sm text-neutral-500">{message}</p> : null}
        </div>
      </section>

      <div className="flex items-center justify-between gap-3">
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="辞書を検索"
          className="w-full max-w-xs rounded-md border border-neutral-200 px-3 py-2 text-sm outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20"
        />
        <p className="text-xs text-neutral-400">{entries.length}件</p>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto rounded-lg border border-neutral-200 bg-white">
        {filtered.length === 0 ? (
          <p className="p-5 text-sm text-neutral-400">登録された単語はありません。</p>
        ) : (
          <ul className="divide-y divide-neutral-100">
            {filtered.map((entry) => (
              <li key={entry.id} className="flex items-center gap-3 px-4 py-3">
                <span
                  className={`size-2 rounded-full ${entry.enabled ? "bg-blue-500" : "bg-neutral-300"}`}
                  aria-hidden
                />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium text-neutral-900">
                    {entry.preferred}
                  </p>
                  <p className="mt-0.5 truncate text-xs text-neutral-400">
                    {[...entry.readings, ...entry.aliases].join(" / ") || "候補なし"}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => editEntry(entry)}
                  className="rounded-md border border-neutral-200 px-2.5 py-1 text-xs text-neutral-600 hover:bg-neutral-50"
                >
                  編集
                </button>
                <button
                  type="button"
                  onClick={() => void removeEntry(entry.id)}
                  className="rounded-md border border-red-100 px-2.5 py-1 text-xs text-red-600 hover:bg-red-50"
                >
                  削除
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
