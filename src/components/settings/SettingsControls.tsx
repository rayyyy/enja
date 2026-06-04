import type { ReactNode } from "react";
import type { PromptCatalogItem, ShortcutBinding } from "../../types";

export const settingsInputClass =
  "w-full rounded-lg bg-neutral-100 px-3 py-2.5 text-sm text-neutral-900 transition placeholder:text-neutral-400 focus:bg-neutral-50 focus:outline-none";

export const settingsSelectClass = settingsInputClass;

export const settingsTextareaClass =
  "w-full resize-y rounded-lg bg-neutral-100 px-3 py-2.5 font-mono text-xs leading-relaxed text-neutral-900 transition focus:bg-neutral-50 focus:outline-none";

export const settingsButtonPrimaryClass =
  "rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white transition hover:bg-blue-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/30 disabled:opacity-50";

export const settingsButtonSecondaryClass =
  "whitespace-nowrap rounded-lg bg-neutral-100 px-3 py-1.5 text-xs font-medium text-neutral-700 transition hover:bg-neutral-200/80 focus-visible:outline-none disabled:opacity-40";

export const settingsButtonAccentClass =
  "whitespace-nowrap rounded-lg bg-blue-100 px-3 py-1.5 text-xs font-medium text-blue-700 transition hover:bg-blue-200/70 focus-visible:outline-none";

export function SettingsSectionPanel({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section className="space-y-6">
      <header className="space-y-1 pb-1">
        <h2 className="text-xl font-semibold tracking-tight text-neutral-900">{title}</h2>
        <p className="max-w-2xl text-sm leading-relaxed text-neutral-500">{description}</p>
      </header>
      <div className="grid gap-5 sm:grid-cols-2">{children}</div>
    </section>
  );
}

export function SettingsFieldGroup({
  className = "",
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return <div className={`flex flex-col gap-3 ${className}`}>{children}</div>;
}

export function ShortcutRow({
  label,
  shortcut,
  capturing,
  onCapture,
  onCancel,
  onReset,
}: {
  label: string;
  shortcut: ShortcutBinding;
  capturing: boolean;
  onCapture: () => void;
  onCancel: () => void;
  onReset: () => void;
}) {
  return (
    <div className="grid gap-4 rounded-xl bg-neutral-100/70 px-4 py-4 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center sm:gap-6">
      <div className="min-w-0">
        <p className="text-sm font-medium text-neutral-900">{label}</p>
        <p className="mt-0.5 text-xs text-neutral-500">
          {capturing ? "キーを押してください。" : "現在の割り当て"}
        </p>
      </div>
      <div className="flex min-w-0 flex-wrap items-center gap-2 sm:flex-nowrap sm:justify-end">
        <kbd
          title={capturing ? undefined : shortcut.label}
          className="inline-flex h-9 min-w-[5.5rem] max-w-[10rem] shrink-0 items-center justify-center truncate rounded-lg bg-white/80 px-3 text-xs font-semibold tracking-wide text-neutral-800"
        >
          {capturing ? "記録中…" : shortcut.label}
        </kbd>
        {capturing ? (
          <button type="button" onClick={onCancel} className={settingsButtonSecondaryClass}>
            中止
          </button>
        ) : (
          <button type="button" onClick={onCapture} className={settingsButtonAccentClass}>
            記録
          </button>
        )}
        <button
          type="button"
          onClick={onReset}
          disabled={capturing}
          className={settingsButtonSecondaryClass}
        >
          デフォルト
        </button>
      </div>
    </div>
  );
}

export function PromptEditor({
  field,
  defaultValue,
  customValue,
  onChange,
}: {
  field: PromptCatalogItem;
  defaultValue: string;
  customValue: string | null;
  onChange: (value: string | null) => void;
}) {
  const customized = customValue !== null;
  const value = customized ? customValue : defaultValue;

  return (
    <div className="overflow-hidden rounded-xl bg-neutral-100/70">
      <div className="flex flex-wrap items-center justify-between gap-3 bg-neutral-100 px-4 py-3">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold text-neutral-900">{field.label}</h3>
          {field.required.length ? (
            <p className="mt-0.5 text-xs text-neutral-500">
              必須: {field.required.join(" / ")}
            </p>
          ) : null}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <label className="flex cursor-pointer items-center gap-2 text-xs text-neutral-600">
            <input
              type="checkbox"
              checked={customized}
              onChange={(e) => onChange(e.target.checked ? defaultValue : null)}
              className="size-4 rounded text-blue-600 focus:ring-0 focus:ring-offset-0"
            />
            カスタム
          </label>
          <button
            type="button"
            onClick={() => onChange(null)}
            className={settingsButtonSecondaryClass}
          >
            デフォルトに戻す
          </button>
        </div>
      </div>
      <textarea
        value={value}
        readOnly={!customized}
        rows={field.rows}
        onChange={(e) => onChange(e.target.value)}
        className={`w-full resize-y px-4 py-3 font-mono text-xs leading-relaxed outline-none ${
          customized ? "bg-white text-neutral-800" : "bg-neutral-50/80 text-neutral-500"
        }`}
      />
    </div>
  );
}

export function Toggle({
  label,
  description,
  checked,
  onChange,
  className = "",
}: {
  label: string;
  description?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  className?: string;
}) {
  return (
    <label
      className={`flex cursor-pointer items-start justify-between gap-4 rounded-xl bg-neutral-100/70 px-4 py-3 transition hover:bg-neutral-100 ${className}`}
    >
      <span className="min-w-0">
        <span className="block text-sm font-medium text-neutral-900">{label}</span>
        {description ? (
          <span className="mt-1 block text-xs leading-relaxed text-neutral-500">
            {description}
          </span>
        ) : null}
      </span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="mt-0.5 size-4 shrink-0 rounded text-blue-600 focus:ring-0 focus:ring-offset-0"
      />
    </label>
  );
}

export function SecretField({
  label,
  placeholder,
  value,
  onChange,
  helpAction,
  helpLabel,
}: {
  label: string;
  placeholder: string;
  value: string;
  onChange: (value: string) => void;
  helpAction?: () => void;
  helpLabel?: string;
}) {
  return (
    <label className="flex flex-col gap-1.5 text-sm">
      <span className="flex items-center justify-between gap-2 font-medium text-neutral-800">
        {label}
        {helpAction ? (
          <button
            type="button"
            onClick={helpAction}
            className="text-xs font-normal text-blue-600 hover:underline"
          >
            {helpLabel}
          </button>
        ) : null}
      </span>
      <input
        type="password"
        autoComplete="off"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className={settingsInputClass}
      />
    </label>
  );
}
