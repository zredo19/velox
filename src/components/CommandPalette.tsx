import { useEffect, useMemo, useRef, useState } from "react";
import {
  Command,
  Eraser,
  Keyboard,
  Moon,
  Palette,
  PanelLeft,
  Play,
  Plus,
  Search,
  Server,
  Settings,
  Sparkles,
  ZoomIn,
  ZoomOut,
  RotateCcw,
} from "lucide-react";
import type { AgentPreset, SessionRecord } from "../types/ipc";
import { THEME_OPTIONS } from "../themes/registry";
import type { ThemePreference } from "../themes/types";
import { AgentIcon } from "./icons/AgentIcons";

export interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  sessions: SessionRecord[];
  presets: AgentPreset[];
  activeSession: SessionRecord | null;
  animatedBackgrounds: boolean;
  themePreference: ThemePreference;
  onSelectSession: (id: string) => void;
  onQuickLaunch: (presetId: string) => void;
  onOpenLauncher: () => void;
  onOpenThemes: () => void;
  onSelectTheme: (pref: ThemePreference) => void;
  onToggleSidebar: () => void;
  onToggleMcpDrawer: () => void;
  onOpenSettings: () => void;
  onOpenShortcuts: () => void;
  onSleepActive: () => void;
  onSleepIdle: () => void;
  onToggleAnimated: () => void;
  onClearTerminal: () => void;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onZoomReset: () => void;
}

interface PaletteItem {
  id: string;
  section: "Sessions" | "Launch Agent" | "Themes" | "Actions";
  label: string;
  subtitle?: string;
  keywords: string;
  shortcut?: string;
  badge?: string;
  icon: React.ReactNode;
  run: () => void;
}

function getFolderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function scoreMatch(query: string, text: string): number {
  if (!query) return 1;
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  if (t.startsWith(q)) return 4;
  if (t.includes(q)) return 3;
  // Subsequence fuzzy match
  let qi = 0;
  for (let i = 0; i < t.length && qi < q.length; i++) {
    if (t[i] === q[qi]) qi++;
  }
  return qi === q.length ? 1 : 0;
}

export function CommandPalette({
  open,
  onClose,
  sessions,
  presets,
  activeSession,
  animatedBackgrounds,
  themePreference,
  onSelectSession,
  onQuickLaunch,
  onOpenLauncher,
  onOpenThemes,
  onSelectTheme,
  onToggleSidebar,
  onToggleMcpDrawer,
  onOpenSettings,
  onOpenShortcuts,
  onSleepActive,
  onSleepIdle,
  onToggleAnimated,
  onClearTerminal,
  onZoomIn,
  onZoomOut,
  onZoomReset,
}: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);

  const items = useMemo<PaletteItem[]>(() => {
    const out: PaletteItem[] = [];

    for (const session of sessions) {
      const kind = session.activeAgent ?? session.agentKind;
      const isSleeping = session.status === "sleeping";
      out.push({
        id: `session:${session.id}`,
        section: "Sessions",
        label: session.title,
        subtitle: getFolderName(session.cwd),
        keywords: `${session.title} ${session.cwd} ${kind} ${session.profile ?? ""} ${session.status}`,
        badge: isSleeping ? "sleeping · 0 MB" : session.status === "exited" ? "exited" : undefined,
        icon: (
          <AgentIcon
            kind={kind}
            className="w-4 h-4 shrink-0"
            active={!isSleeping && session.status !== "exited"}
          />
        ),
        run: () => onSelectSession(session.id),
      });
    }

    for (const preset of presets) {
      out.push({
        id: `launch:${preset.id}`,
        section: "Launch Agent",
        label: `New ${preset.label}`,
        subtitle: preset.profile ? `${preset.profile} · ${preset.description}` : preset.description,
        keywords: `new launch start ${preset.label} ${preset.kind} ${preset.profile ?? ""}`,
        icon: <AgentIcon kind={preset.kind} className="w-4 h-4 shrink-0" active />,
        run: () => onQuickLaunch(preset.id),
      });
    }

    out.push({
      id: "action:launcher",
      section: "Actions",
      label: "Open Agent Launcher…",
      subtitle: "Pick agent, project folder, and custom label",
      keywords: "new session launcher folder agent",
      shortcut: "Ctrl+N",
      icon: <Plus className="w-4 h-4 text-[var(--velox-accent-soft)]" />,
      run: onOpenLauncher,
    });

    if (activeSession && activeSession.status !== "sleeping") {
      out.push({
        id: "action:sleep-active",
        section: "Actions",
        label: `Sleep "${activeSession.title}"`,
        subtitle: "Hibernate current session to 0 MB RAM",
        keywords: "sleep hibernate pause ram current session",
        shortcut: "Ctrl+Shift+H",
        icon: <Moon className="w-4 h-4 text-[var(--velox-ink-secondary)]" />,
        run: onSleepActive,
      });
      out.push({
        id: "action:clear",
        section: "Actions",
        label: "Clear terminal scrollback",
        subtitle: "Clear the active terminal screen",
        keywords: "clear reset terminal screen",
        icon: <Eraser className="w-4 h-4 text-[var(--velox-ink-secondary)]" />,
        run: onClearTerminal,
      });
    } else if (activeSession && activeSession.status === "sleeping") {
      out.push({
        id: "action:wake-active",
        section: "Actions",
        label: `Wake "${activeSession.title}"`,
        subtitle: "Resume hibernated session",
        keywords: "wake resume start sleeping session",
        icon: <Play className="w-4 h-4 text-[var(--velox-accent-soft)]" />,
        run: () => onSelectSession(activeSession.id),
      });
    }

    out.push(
      {
        id: "action:sleep-idle",
        section: "Actions",
        label: "Sleep all idle sessions",
        subtitle: "Free RAM across all background tabs",
        keywords: "sleep idle hibernate all tabs memory ram",
        icon: <Moon className="w-4 h-4 text-[var(--velox-accent-soft)]" />,
        run: onSleepIdle,
      },
      {
        id: "action:sidebar",
        section: "Actions",
        label: "Toggle sidebar",
        subtitle: "Show or hide the sessions panel",
        keywords: "sidebar panel toggle hide show",
        shortcut: "Ctrl+Shift+B",
        icon: <PanelLeft className="w-4 h-4 text-[var(--velox-ink-secondary)]" />,
        run: onToggleSidebar,
      },
      {
        id: "action:mcp",
        section: "Actions",
        label: "Toggle Lazy MCP Gateway drawer",
        subtitle: "Inspect active MCP backends and RAM",
        keywords: "mcp gateway drawer backends memory",
        shortcut: "Ctrl+Shift+M",
        icon: <Server className="w-4 h-4 text-[var(--velox-ink-secondary)]" />,
        run: onToggleMcpDrawer,
      },
      {
        id: "action:themes",
        section: "Actions",
        label: "Open Theme Gallery…",
        subtitle: "Browse custom themes and animated wallpapers",
        keywords: "theme palette wallpaper color appearance custom",
        shortcut: "Ctrl+Shift+T",
        icon: <Palette className="w-4 h-4 text-[var(--velox-accent-soft)]" />,
        run: onOpenThemes,
      },
      {
        id: "action:animated",
        section: "Actions",
        label: animatedBackgrounds
          ? "Disable animated backgrounds"
          : "Enable animated backgrounds",
        subtitle: "Toggle OffscreenCanvas agent wallpapers",
        keywords: "theme wallpaper animation background performance",
        icon: <Sparkles className="w-4 h-4 text-[var(--velox-accent-soft)]" />,
        run: onToggleAnimated,
      },
      {
        id: "action:zoom-in",
        section: "Actions",
        label: "Zoom in terminal font",
        keywords: "font size larger zoom in bigger",
        shortcut: "Ctrl+=",
        icon: <ZoomIn className="w-4 h-4 text-[var(--velox-ink-secondary)]" />,
        run: onZoomIn,
      },
      {
        id: "action:zoom-out",
        section: "Actions",
        label: "Zoom out terminal font",
        keywords: "font size smaller zoom out",
        shortcut: "Ctrl+-",
        icon: <ZoomOut className="w-4 h-4 text-[var(--velox-ink-secondary)]" />,
        run: onZoomOut,
      },
      {
        id: "action:zoom-reset",
        section: "Actions",
        label: "Reset terminal font size (13px)",
        keywords: "font size reset default 13",
        shortcut: "Ctrl+0",
        icon: <RotateCcw className="w-4 h-4 text-[var(--velox-ink-secondary)]" />,
        run: onZoomReset,
      },
      {
        id: "action:settings",
        section: "Actions",
        label: "Open Settings",
        subtitle: "Configure hibernation, scrollback, MCP & fonts",
        keywords: "settings preferences config options",
        shortcut: "Ctrl+,",
        icon: <Settings className="w-4 h-4 text-[var(--velox-ink-secondary)]" />,
        run: onOpenSettings,
      },
      {
        id: "action:shortcuts",
        section: "Actions",
        label: "Keyboard shortcuts",
        subtitle: "View all Velox keyboard shortcuts",
        keywords: "shortcuts hotkeys keys help cheat sheet",
        shortcut: "Ctrl+/",
        icon: <Keyboard className="w-4 h-4 text-[var(--velox-ink-secondary)]" />,
        run: onOpenShortcuts,
      },
    );

    for (const meta of THEME_OPTIONS) {
      out.push({
        id: `theme:${meta.id}`,
        section: "Themes",
        label: `Theme: ${meta.label}`,
        subtitle: meta.description,
        keywords: `theme color wallpaper ${meta.label} ${meta.id} ${meta.description}`,
        badge: themePreference === meta.id ? "active" : undefined,
        icon: (
          <span
            className="w-3.5 h-3.5 rounded-full border border-white/20"
            style={{ backgroundColor: meta.swatches[2] }}
          />
        ),
        run: () => onSelectTheme(meta.id),
      });
    }

    return out;
  }, [
    sessions,
    presets,
    activeSession,
    animatedBackgrounds,
    themePreference,
    onSelectSession,
    onQuickLaunch,
    onOpenLauncher,
    onOpenThemes,
    onSelectTheme,
    onToggleSidebar,
    onToggleMcpDrawer,
    onOpenSettings,
    onOpenShortcuts,
    onSleepActive,
    onSleepIdle,
    onToggleAnimated,
    onClearTerminal,
    onZoomIn,
    onZoomOut,
    onZoomReset,
  ]);

  const filtered = useMemo(() => {
    const q = query.trim();
    if (!q) return items;
    return items
      .map((item) => ({
        item,
        score: Math.max(scoreMatch(q, item.label) * 2, scoreMatch(q, item.keywords)),
      }))
      .filter((entry) => entry.score > 0)
      .sort((a, b) => b.score - a.score)
      .map((entry) => entry.item);
  }, [items, query]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setSelectedIndex(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => {
    setSelectedIndex((prev) => Math.min(prev, Math.max(0, filtered.length - 1)));
  }, [filtered.length]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIndex((i) => (filtered.length ? (i + 1) % filtered.length : 0));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIndex((i) =>
          filtered.length ? (i - 1 + filtered.length) % filtered.length : 0,
        );
      } else if (e.key === "Enter") {
        const chosen = filtered[selectedIndex];
        if (chosen) {
          e.preventDefault();
          onClose();
          chosen.run();
        }
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [open, filtered, selectedIndex, onClose]);

  // Keep selected item scrolled into view
  useEffect(() => {
    if (!open) return;
    const activeEl = listRef.current?.querySelector<HTMLElement>(
      `[data-palette-idx="${selectedIndex}"]`,
    );
    activeEl?.scrollIntoView({ block: "nearest" });
  }, [open, selectedIndex]);

  if (!open) return null;

  let lastSection = "";

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[12vh] bg-black/55 backdrop-blur-[2px] p-4 animate-velox-fade-in"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-label="Command palette"
    >
      <div className="w-full max-w-[540px] rounded-xl bg-[var(--velox-surface)] border border-white/[0.11] shadow-[0_28px_70px_-20px_rgba(0,0,0,0.85)] overflow-hidden animate-velox-pop-in">
        {/* Search input */}
        <div className="flex items-center gap-2.5 px-4 h-12 border-b border-white/[0.08]">
          <Search className="w-4 h-4 text-[var(--velox-ink-muted)] shrink-0" />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSelectedIndex(0);
            }}
            placeholder="Jump to session, launch agent, or run command..."
            className="flex-1 bg-transparent text-[13.5px] text-[var(--velox-ink)] placeholder:text-[var(--velox-ink-muted)] outline-none"
          />
          <span className="velox-kbd">Esc</span>
        </div>

        {/* Results */}
        <div ref={listRef} className="max-h-[380px] overflow-y-auto p-1.5">
          {filtered.length === 0 ? (
            <div className="py-10 text-center text-[12.5px] text-[var(--velox-ink-muted)]">
              No matching sessions or commands
            </div>
          ) : (
            filtered.map((item, idx) => {
              const showHeader = !query.trim() && item.section !== lastSection;
              lastSection = item.section;
              const selected = idx === selectedIndex;

              return (
                <div key={item.id}>
                  {showHeader && (
                    <div className="px-2.5 pt-2 pb-1 text-[10.5px] font-medium uppercase tracking-wider text-[var(--velox-ink-muted)]">
                      {item.section}
                    </div>
                  )}
                  <button
                    type="button"
                    data-palette-idx={idx}
                    onMouseEnter={() => setSelectedIndex(idx)}
                    onClick={() => {
                      onClose();
                      item.run();
                    }}
                    className={`w-full px-2.5 py-2 rounded-lg flex items-center gap-2.5 text-left transition-colors cursor-pointer ${
                      selected
                        ? "bg-[var(--velox-active)] text-[var(--velox-ink)]"
                        : "text-[var(--velox-ink-secondary)] hover:bg-[var(--velox-hover)]"
                    }`}
                  >
                    <div className="w-6 h-6 rounded-md bg-[var(--velox-canvas)] border border-white/[0.07] flex items-center justify-center shrink-0">
                      {item.icon}
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 min-w-0">
                        <span className="text-[12.5px] font-medium text-[var(--velox-ink)] truncate">
                          {item.label}
                        </span>
                        {item.badge && (
                          <span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-white/[0.06] text-[var(--velox-ink-soft)] shrink-0">
                            {item.badge}
                          </span>
                        )}
                      </div>
                      {item.subtitle && (
                        <div className="text-[11px] text-[var(--velox-ink-muted)] truncate">
                          {item.subtitle}
                        </div>
                      )}
                    </div>
                    {item.shortcut && <span className="velox-kbd shrink-0">{item.shortcut}</span>}
                  </button>
                </div>
              );
            })
          )}
        </div>

        {/* Footer hints */}
        <div className="px-3.5 py-2 border-t border-white/[0.07] bg-[var(--velox-canvas)]/55 flex items-center justify-between text-[11px] text-[var(--velox-ink-muted)]">
          <div className="flex items-center gap-3">
            <span className="inline-flex items-center gap-1">
              <span className="velox-kbd">↑↓</span> navigate
            </span>
            <span className="inline-flex items-center gap-1">
              <span className="velox-kbd">↵</span> run
            </span>
          </div>
          <span className="inline-flex items-center gap-1 font-mono text-[10.5px]">
            <Command className="w-3 h-3" />
            Velox Palette
          </span>
        </div>
      </div>
    </div>
  );
}
