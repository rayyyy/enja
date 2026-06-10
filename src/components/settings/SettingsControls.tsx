import { memo, type ReactNode } from "react";
import type { PromptCatalogItem, PromptOverrides, ShortcutBinding } from "../../types";

export const settingsInputClass =
  "w-full rounded-lg border border-edge bg-sunken px-3 py-2 text-sm text-ink transition-[border-color,box-shadow,background-color] duration-100 placeholder:text-ink-faint focus:border-accent focus:bg-surface focus:outline-none focus:ring-2 focus:ring-accent/25";

export const settingsSelectClass = settingsInputClass;

export const settingsTextareaClass =
  "w-full resize-y rounded-lg border border-edge bg-sunken px-3 py-2.5 font-mono text-xs leading-relaxed text-ink transition-[border-color,box-shadow,background-color] duration-100 focus:border-accent focus:bg-surface focus:outline-none focus:ring-2 focus:ring-accent/25";

export const settingsButtonPrimaryClass =
  "rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-100 focus-ring hover:bg-accent-deep active:scale-[0.98] disabled:opacity-50";

export const settingsButtonSecondaryClass =
  "whitespace-nowrap rounded-lg border border-edge bg-surface px-3 py-1.5 text-xs font-medium text-ink-mid transition-colors duration-100 focus-ring hover:bg-hover hover:text-ink disabled:opacity-40";

export const settingsButtonAccentClass =
  "whitespace-nowrap rounded-lg bg-accent-soft px-3 py-1.5 text-xs font-medium text-accent-ink transition-colors duration-100 focus-ring hover:bg-accent/20";

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
        <h2 className="text-xl font-semibold tracking-tight text-ink">{title}</h2>
        <p className="max-w-2xl text-sm leading-relaxed text-ink-mid">{description}</p>
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

export const ShortcutRow = memo(function ShortcutRow({
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
    <div className="grid gap-4 rounded-xl border border-edge bg-sunken px-4 py-4 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center sm:gap-6">
      <div className="min-w-0">
        <p className="text-sm font-medium text-ink">{label}</p>
        <p className="mt-0.5 text-xs text-ink-mid">
          {capturing ? "キーを押してください。" : "現在の割り当て"}
        </p>
      </div>
      <div className="flex min-w-0 flex-wrap items-center gap-2 sm:flex-nowrap sm:justify-end">
        <kbd
          title={capturing ? undefined : shortcut.label}
          className="inline-flex h-9 min-w-[5.5rem] max-w-[10rem] shrink-0 items-center justify-center truncate rounded-lg border border-edge bg-surface px-3 text-xs font-semibold tracking-wide text-ink"
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
});

export const PromptEditor = memo(function PromptEditor({
  field,
  defaultValue,
  customValue,
  onChange,
}: {
  field: PromptCatalogItem;
  defaultValue: string;
  customValue: string | null;
  onChange: (key: keyof PromptOverrides, value: string | null) => void;
}) {
  const customized = customValue !== null;
  const value = customized ? customValue : defaultValue;

  return (
    <div className="overflow-hidden rounded-xl border border-edge bg-sunken">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-edge px-4 py-3">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold text-ink">{field.label}</h3>
          {field.required.length ? (
            <p className="mt-0.5 text-xs text-ink-mid">
              必須: {field.required.join(" / ")}
            </p>
          ) : null}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <label className="flex cursor-pointer items-center gap-2 text-xs text-ink-mid">
            <input
              type="checkbox"
              checked={customized}
              onChange={(e) => onChange(field.key, e.target.checked ? defaultValue : null)}
              className="size-4 rounded accent-[var(--accent)] focus:ring-0 focus:ring-offset-0"
            />
            カスタム
          </label>
          <button
            type="button"
            onClick={() => onChange(field.key, null)}
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
        onChange={(e) => onChange(field.key, e.target.value)}
        className={`w-full resize-y px-4 py-3 font-mono text-xs leading-relaxed outline-none ${
          customized ? "bg-surface text-ink" : "bg-sunken text-ink-mid"
        }`}
      />
    </div>
  );
});

export const Toggle = memo(function Toggle({
  label,
  description,
  checked,
  onChange,
  disabled = false,
  className = "",
}: {
  label: string;
  description?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <label
      className={`flex items-start justify-between gap-4 rounded-xl border border-edge bg-sunken px-4 py-3 transition-colors duration-100 ${
        disabled ? "cursor-not-allowed opacity-60" : "cursor-pointer hover:bg-hover"
      } ${className}`}
    >
      <span className="min-w-0">
        <span className="block text-sm font-medium text-ink">{label}</span>
        {description ? (
          <span className="mt-1 block text-xs leading-relaxed text-ink-mid">
            {description}
          </span>
        ) : null}
      </span>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
        className="mt-0.5 size-4 shrink-0 rounded accent-[var(--accent)] focus:ring-0 focus:ring-offset-0"
      />
    </label>
  );
});

export const SecretField = memo(function SecretField({
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
      <span className="flex items-center justify-between gap-2 font-medium text-ink">
        {label}
        {helpAction ? (
          <button
            type="button"
            onClick={helpAction}
            className="text-xs font-normal text-accent-ink hover:underline"
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
});
