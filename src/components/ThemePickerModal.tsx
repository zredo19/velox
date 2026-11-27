import { useEffect } from "react";
import { Check, Palette, Sparkles, X } from "lucide-react";
import { THEME_OPTIONS } from "../themes/registry";
import type { ThemeMeta, ThemePreference } from "../themes/types";

export interface ThemePickerModalProps {
  open: boolean;
  onClose: () => void;
  selected: ThemePreference;
  animatedBackgrounds: boolean;
  onSelectTheme: (pref: ThemePreference) => void;
  onToggleAnimated: () => void;
}

function ThemeCard({
  meta,
  active,
  onClick,
}: {
  meta: ThemeMeta;
  active: boolean;
  onClick: () => void;
}) {
  const [canvas, surface, accent, accentSoft] = meta.swatches;

  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        borderColor: active ? accent : "rgba(255, 255, 255, 0.08)",
        boxShadow: active
          ? `0 0 0 1px color-mix(in srgb, ${accent} 40%, transparent), 0 10px 24px -14px ${accent}`
          : undefined,
      }}
      className={`group relative text-left p-2.5 rounded-xl border transition-all duration-150 active:scale-[0.98] cursor-pointer ${
        active
          ? "bg-[var(--velox-active)]"
          : "bg-[var(--velox-canvas)]/70 hover:bg-[var(--velox-elevated)]"
      }`}
    >
      {/* Mini terminal window preview */}
      <div
        className="relative h-12 w-full rounded-lg border border-white/[0.08] overflow-hidden flex"
        style={{ backgroundColor: canvas }}
      >
        {/* Mini sidebar */}
        <div
          className="w-7 h-full border-r border-white/[0.07] p-1 flex flex-col gap-1"
          style={{ backgroundColor: surface }}
        >
          <span className="h-1.5 w-full rounded-full" style={{ backgroundColor: accent }} />
          <span className="h-1 w-3/4 rounded-full bg-white/15" />
          <span className="h-1 w-2/3 rounded-full bg-white/10" />
        </div>
        {/* Mini terminal lines + glow orb */}
        <div className="relative flex-1 p-1.5 flex flex-col justify-center gap-1">
          <div
            aria-hidden="true"
            className="pointer-events-none absolute -right-2 -top-2 w-10 h-10 rounded-full opacity-45 blur-md"
            style={{ backgroundColor: accent }}
          />
          <div className="flex items-center gap-1">
            <span className="h-1 w-2 rounded-full" style={{ backgroundColor: accent }} />
            <span className="h-1 w-8 rounded-full" style={{ backgroundColor: accentSoft }} />
          </div>
          <div className="h-1 w-11 rounded-full bg-white/25" />
          <div className="h-1 w-7 rounded-full bg-white/15" />
        </div>

        {active && (
          <span
            className="absolute top-1.5 right-1.5 w-4 h-4 rounded-full flex items-center justify-center text-[#080B12] shadow"
            style={{ backgroundColor: accent }}
          >
            <Check className="w-2.5 h-2.5 stroke-[3]" />
          </span>
        )}
      </div>

      {/* Label & color dots */}
      <div className="mt-2 flex items-center justify-between gap-1.5">
        <span className="text-[12px] font-medium text-[var(--velox-ink)] truncate">
          {meta.label}
        </span>
        <div className="flex items-center gap-1 shrink-0">
          <span
            className="w-2 h-2 rounded-full border border-white/15"
            style={{ backgroundColor: accent }}
          />
          <span
            className="w-2 h-2 rounded-full border border-white/15"
            style={{ backgroundColor: accentSoft }}
          />
        </div>
      </div>
      <p className="mt-0.5 text-[10.5px] text-[var(--velox-ink-muted)] truncate">
        {meta.description}
      </p>
    </button>
  );
}

export function ThemePickerModal({
  open,
  onClose,
  selected,
  animatedBackgrounds,
  onSelectTheme,
  onToggleAnimated,
}: ThemePickerModalProps) {
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

  const autoOpt = THEME_OPTIONS.find((t) => t.category === "auto");
  const customOpts = THEME_OPTIONS.filter((t) => t.category === "custom");
  const agentOpts = THEME_OPTIONS.filter((t) => t.category === "agent");

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-[2px] p-4 animate-velox-fade-in"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="velox-themes-title"
    >
      <div className="w-full max-w-[660px] max-h-[86vh] bg-[var(--velox-surface)] border border-white/[0.11] rounded-xl shadow-[0_28px_70px_-20px_rgba(0,0,0,0.85)] flex flex-col overflow-hidden animate-velox-pop-in">
        {/* Header */}
        <div className="px-5 py-3.5 border-b border-white/[0.07] flex items-center justify-between gap-3">
          <div className="flex items-center gap-2">
            <Palette className="w-4 h-4 text-[var(--velox-accent-soft)]" />
            <div>
              <h2
                id="velox-themes-title"
                className="text-[14.5px] font-medium text-[var(--velox-ink)]"
              >
                Themes & wallpapers
              </h2>
              <p className="text-[11.5px] text-[var(--velox-ink-soft)]">
                Switch live between dynamic per-agent themes or pin a custom theme
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={onToggleAnimated}
              className={`h-7 px-2.5 rounded-lg border text-[11.5px] font-medium inline-flex items-center gap-1.5 transition-colors cursor-pointer ${
                animatedBackgrounds
                  ? "bg-[var(--velox-accent)]/15 border-[var(--velox-accent)]/35 text-[var(--velox-accent-soft)]"
                  : "bg-[var(--velox-canvas)] border-white/[0.08] text-[var(--velox-ink-muted)] hover:text-[var(--velox-ink)]"
              }`}
            >
              <Sparkles className="w-3 h-3" />
              <span>{animatedBackgrounds ? "Animated ON" : "Animated OFF"}</span>
            </button>

            <button
              type="button"
              onClick={onClose}
              aria-label="Close themes"
              className="h-7 w-7 flex items-center justify-center rounded-md text-[var(--velox-ink-soft)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] transition-colors cursor-pointer"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto p-5 space-y-5">
          {autoOpt && (
            <section>
              <h3 className="text-[11px] font-medium uppercase tracking-wider text-[var(--velox-ink-muted)] mb-2">
                Dynamic
              </h3>
              <div className="grid grid-cols-1">
                <ThemeCard
                  meta={autoOpt}
                  active={selected === autoOpt.id}
                  onClick={() => onSelectTheme(autoOpt.id)}
                />
              </div>
            </section>
          )}

          <section>
            <h3 className="text-[11px] font-medium uppercase tracking-wider text-[var(--velox-ink-muted)] mb-2">
              Custom themes
            </h3>
            <div className="grid grid-cols-2 sm:grid-cols-3 gap-2.5">
              {customOpts.map((meta) => (
                <ThemeCard
                  key={meta.id}
                  meta={meta}
                  active={selected === meta.id}
                  onClick={() => onSelectTheme(meta.id)}
                />
              ))}
            </div>
          </section>

          <section>
            <h3 className="text-[11px] font-medium uppercase tracking-wider text-[var(--velox-ink-muted)] mb-2">
              Agent themes (pin globally)
            </h3>
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-2.5">
              {agentOpts.map((meta) => (
                <ThemeCard
                  key={meta.id}
                  meta={meta}
                  active={selected === meta.id}
                  onClick={() => onSelectTheme(meta.id)}
                />
              ))}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
