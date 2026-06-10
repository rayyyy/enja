import type { ButtonHTMLAttributes, ReactNode } from "react";

/** キーボードヒント用チップ（⌘K風） */
export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-flex h-[18px] min-w-[18px] items-center justify-center rounded border border-edge bg-sunken px-1.5 font-sans text-[10px] font-medium leading-none text-ink-mid">
      {children}
    </kbd>
  );
}

/** ツールバー用ゴーストアイコンボタン */
export function IconButton({
  className = "",
  active = false,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { active?: boolean }) {
  return (
    <button
      type="button"
      {...props}
      className={`grid size-7 place-items-center rounded-md transition-colors duration-100 focus-ring disabled:pointer-events-none disabled:opacity-40 ${
        active
          ? "bg-accent-soft text-accent-ink"
          : "text-ink-faint hover:bg-hover hover:text-ink"
      } ${className}`}
    />
  );
}
