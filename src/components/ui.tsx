import type {
  ButtonHTMLAttributes,
  HTMLAttributes,
  MouseEvent,
  ReactNode,
} from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

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

export function WindowDragRegion({
  className = "",
  ...props
}: HTMLAttributes<HTMLDivElement>) {
  function handleMouseDown(event: MouseEvent<HTMLDivElement>) {
    props.onMouseDown?.(event);
    if (event.defaultPrevented || event.button !== 0) return;
    if (isInteractiveDragTarget(event.target)) return;
    void getCurrentWindow().startDragging().catch(() => undefined);
  }

  return (
    <div
      data-tauri-drag-region
      {...props}
      onMouseDown={handleMouseDown}
      className={`window-drag-region ${className}`}
    />
  );
}

function isInteractiveDragTarget(target: EventTarget | null) {
  return (
    target instanceof Element &&
    Boolean(
      target.closest(
        "button,input,textarea,select,a,[contenteditable=true],[data-window-no-drag]",
      ),
    )
  );
}
