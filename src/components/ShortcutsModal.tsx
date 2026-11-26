import { useEffect } from "react";
import { Keyboard, X } from "lucide-react";

export interface ShortcutsModalProps {
  open: boolean;
  onClose: () => void;
}

const GROUPS = [
  {
    title: "Navigation & Palette",
    items: [
      { keys: ["Ctrl", "K"], label: "Command palette" },
      { keys: ["Ctrl", "1–9"], label: "Switch to session 1–9" },
      { keys: ["Ctrl", "Shift", "B"], label: "Toggle sidebar" },
      { keys: ["Ctrl", "Shift", "M"], label: "Lazy MCP Gateway drawer" },
      { keys: ["Ctrl", ","], label: "Open settings" },
      { keys: ["Ctrl", "/"], label: "Keyboard shortcuts" },
    ],
  },
  {
    title: "Sessions",
    items: [
      { keys: ["Ctrl", "N"], label: "Launch new agent session" },
      { keys: ["Ctrl", "Shift", "H"], label: "Hibernate active session (0 MB)" },
      { keys: ["Ctrl", "W"], label: "Close session (outside terminal)" },
      { keys: ["Double-click"], label: "Rename session in sidebar" },
    ],
  },
  {
    title: "Terminal & Zoom",
    items: [
      { keys: ["Ctrl", "Shift", "C"], label: "Copy selection" },
      { keys: ["Ctrl", "V"], label: "Paste text or clipboard images" },
      { keys: ["Ctrl", "="], label: "Increase terminal font size" },
      { keys: ["Ctrl", "-"], label: "Decrease terminal font size" },
      { keys: ["Ctrl", "0"], label: "Reset terminal font size" },
    ],
  },
];

export function ShortcutsModal({ open, onClose }: ShortcutsModalProps) {
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-[2px] p-4 animate-velox-fade-in"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="velox-shortcuts-title"
    >
      <div className="w-full max-w-[500px] bg-[var(--velox-surface)] border border-white/[0.1] rounded-xl shadow-2xl overflow-hidden animate-velox-pop-in">
        <div className="px-5 py-3.5 border-b border-white/[0.07] flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Keyboard className="w-4 h-4 text-[var(--velox-accent-soft)]" />
            <h2
              id="velox-shortcuts-title"
              className="text-[14.5px] font-medium text-[var(--velox-ink)]"
            >
              Keyboard shortcuts
            </h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close keyboard shortcuts"
            className="h-7 w-7 flex items-center justify-center rounded-md text-[var(--velox-ink-soft)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] transition-colors cursor-pointer"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="p-5 space-y-5 max-h-[75vh] overflow-y-auto">
          {GROUPS.map((group) => (
            <div key={group.title}>
              <h3 className="text-[11px] font-medium uppercase tracking-wider text-[var(--velox-ink-muted)] mb-2">
                {group.title}
              </h3>
              <div className="space-y-1.5">
                {group.items.map((item) => (
                  <div
                    key={item.label}
                    className="flex items-center justify-between px-3 py-1.5 rounded-lg bg-[var(--velox-canvas)]/60 border border-white/[0.05]"
                  >
                    <span className="text-[12.5px] text-[var(--velox-ink-secondary)]">
                      {item.label}
                    </span>
                    <div className="flex items-center gap-1">
                      {item.keys.map((k) => (
                        <span key={k} className="velox-kbd">
                          {k}
                        </span>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
