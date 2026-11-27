import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { Folder, Moon, Play, Sparkles } from "lucide-react";
import { decodeBase64ToBytes, veloxApi } from "../lib/tauri";
import type { PtyOutputEvent, SessionRecord } from "../types/ipc";
import { AgentIcon } from "./icons/AgentIcons";
import type { AgentTheme } from "../themes/types";
import { useAgentActivity, type ReportActivity } from "../lib/useAgentActivity";
import { useTerminalPaste } from "../lib/useTerminalPaste";
import { fixTransparentCellBackgrounds } from "../lib/xtermTransparencyFix";

export interface TerminalPaneProps {
  session: SessionRecord;
  isActive: boolean;
  fontFamily: string;
  fontSize: number;
  theme: AgentTheme;
  clearTrigger?: number;
  onActivity: ReportActivity;
  onWake: (id: string, cols: number, rows: number) => void;
}

/** A hidden pane keeps its WebGL context this long, so quick tab switching stays instant. */
const WEBGL_RELEASE_DELAY_MS = 30_000;

function getCwdBasename(cwd: string): string {
  const parts = cwd.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? cwd;
}

function concatBytes(chunks: Uint8Array[]): Uint8Array {
  if (chunks.length === 1) return chunks[0];
  let total = 0;
  for (const c of chunks) total += c.length;
  const merged = new Uint8Array(total);
  let offset = 0;
  for (const c of chunks) {
    merged.set(c, offset);
    offset += c.length;
  }
  return merged;
}

// Memoized: App re-renders on every telemetry tick, and every pane re-rendering with it is
// wasted work. Props are stable (themes are shared objects, callbacks are memoized).
export const TerminalPane = memo(function TerminalPane({
  session,
  isActive,
  fontFamily,
  fontSize,
  theme,
  clearTrigger = 0,
  onActivity,
  onWake,
}: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const sleepRootRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const resizeTimerRef = useRef<number | null>(null);
  const webglRef = useRef<WebglAddon | null>(null);
  const copyNoticeTimerRef = useRef<number | undefined>(undefined);
  const lastSentSizeRef = useRef<{ cols: number; rows: number } | null>(null);
  const [copyNotice, setCopyNotice] = useState<string | null>(null);

  const isSleeping = session.status === "sleeping";
  const effectiveKind = session.activeAgent ?? session.agentKind;
  const effectiveKindRef = useRef(effectiveKind);
  effectiveKindRef.current = effectiveKind;
  const isActiveRef = useRef(isActive);
  isActiveRef.current = isActive;

  const titleRef = useAgentActivity(
    session.id,
    effectiveKind,
    !isSleeping,
    termRef,
    onActivity,
  );
  const pasteNotice = useTerminalPaste(
    session.id,
    isActive && !isSleeping,
    !isSleeping,
    containerRef,
    termRef,
  );
  const paneNotice = pasteNotice ?? copyNotice;

  const showCopyNotice = useCallback((message: string) => {
    window.clearTimeout(copyNoticeTimerRef.current);
    setCopyNotice(message);
    copyNoticeTimerRef.current = window.setTimeout(() => {
      setCopyNotice(null);
    }, 1800);
  }, []);

  // The only way a pane is resized. A hidden pane must be left alone: measuring it yields
  // a few columns, and that size would be pushed to the running process.
  const fitIfVisible = useCallback(
    (forceResizeIpc = false) => {
      const container = containerRef.current;
      const term = termRef.current;
      if (!container || !term || container.clientWidth < 40 || container.clientHeight < 40) return;
      try {
        fitAddonRef.current?.fit();
        // xterm 5.5 computes _scrollArea height from viewport.offsetHeight (0 while display:none)
        // and fit() skips resize when cols/rows match, leaving scrollTop/scrollHeight stale.
        const viewport = (
          term as unknown as {
            _core?: {
              viewport?: {
                _ignoreNextScrollEvent?: boolean;
                _lastRecordedViewportHeight?: number;
                syncScrollArea: (immediate?: boolean) => void;
              };
            };
          }
        )._core?.viewport;
        if (viewport) {
          viewport._ignoreNextScrollEvent = false;
          viewport._lastRecordedViewportHeight = -1;
          viewport.syncScrollArea(true);
        }
        const prev = lastSentSizeRef.current;
        if (forceResizeIpc || !prev || prev.cols !== term.cols || prev.rows !== term.rows) {
          lastSentSizeRef.current = { cols: term.cols, rows: term.rows };
          void veloxApi.resizePty(session.id, term.cols, term.rows);
        }
      } catch {
        // Ignore transient resize errors
      }
    },
    [session.id],
  );

  const handleWake = useCallback(() => {
    const el = sleepRootRef.current;
    let cols = 120;
    let rows = 34;
    if (el && el.clientWidth > 120 && el.clientHeight > 120) {
      const approxCellW = Math.max(6, fontSize * 0.62);
      const approxCellH = Math.max(12, fontSize * 1.24);
      cols = Math.max(40, Math.floor((el.clientWidth - 28) / approxCellW));
      rows = Math.max(12, Math.floor((el.clientHeight - 20) / approxCellH));
    }
    onWake(session.id, cols, rows);
  }, [session.id, fontSize, onWake]);

  // Clear terminal screen when triggered from Command Palette
  useEffect(() => {
    if (clearTrigger > 0 && isActive && !isSleeping && termRef.current) {
      termRef.current.clear();
      termRef.current.focus();
    }
  }, [clearTrigger, isActive, isSleeping]);

  // Mount xterm.js only while awake; unmount when sleeping to release WebGL + DOM memory
  useEffect(() => {
    if (isSleeping) {
      lastSentSizeRef.current = null;
      return;
    }

    const container = containerRef.current;
    if (!container) return;

    let disposed = false;
    let unlistenOutput: (() => void) | undefined;
    lastSentSizeRef.current = null;

    const term = new Terminal({
      theme: theme.xterm,
      // Fixed at construction: agent panes need it so the animated canvas can show through.
      allowTransparency: theme.id !== "default",
      cursorBlink: true,
      cursorStyle: "bar",
      scrollback: 5000,
      allowProposedApi: true,
      fontFamily,
      fontSize,
      lineHeight: 1.24,
      letterSpacing: 0,
      tabStopWidth: 4,
    });

    const fitAddon = new FitAddon();
    const unicode11 = new Unicode11Addon();
    term.loadAddon(fitAddon);
    term.loadAddon(new WebLinksAddon());
    term.loadAddon(unicode11);
    term.unicode.activeVersion = "11";

    term.onTitleChange((title) => {
      titleRef.current = title;
    });

    term.open(container);
    // The WebGL renderer is attached by the visibility effect below, only while on screen.

    // Clipboard ergonomics: Ctrl+V / Ctrl+Shift+V paste, Ctrl+Shift+C copy, Ctrl+C smart copy/SIGINT
    term.attachCustomKeyEventHandler((ev: KeyboardEvent) => {
      if (ev.type !== "keydown") return true;

      if (ev.ctrlKey && ev.shiftKey && ev.key.toLowerCase() === "c") {
        const selection = term.getSelection();
        if (selection) {
          void navigator.clipboard.writeText(selection).catch(() => {});
          showCopyNotice("Copied to clipboard");
        }
        return false;
      }

      // Not forwarded to the PTY: the browser then raises a native paste event, which
      // xterm handles for text and useTerminalPaste handles for images.
      if (ev.ctrlKey && !ev.altKey && ev.key.toLowerCase() === "v") {
        return false;
      }

      if (ev.ctrlKey && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "c") {
        const selection = term.getSelection();
        if (selection) {
          void navigator.clipboard.writeText(selection).catch(() => {});
          term.clearSelection();
          showCopyNotice("Copied to clipboard");
          return false;
        }
        return true;
      }

      // Velox shortcuts: not sent to the PTY, handled by the window listener in App.
      // Plain Ctrl+B/M/H stay with the terminal program (Claude Code uses Ctrl+B; Ctrl+M is
      // Enter and Ctrl+H Backspace). Plain Ctrl+N, Ctrl+K, Ctrl+/, Ctrl+=/-/0 are Velox shortcuts.
      const key = ev.key.toLowerCase();
      if (
        ev.ctrlKey &&
        !ev.altKey &&
        (ev.shiftKey
          ? ["n", "b", "m", "h", "p", "t"].includes(key)
          : /^[n,k/0=+-1-9]$/.test(key))
      ) {
        return false;
      }

      return true;
    });

    termRef.current = term;
    fitAddonRef.current = fitAddon;

    fitIfVisible(true);
    if (isActiveRef.current) {
      term.focus();
    }

    const dataDisposable = term.onData((data) => {
      void veloxApi.writePty(session.id, data);
    });

    let mouseDragging = false;
    let selectionChangedDuringDrag = false;
    let copiedOnMouseUp = false;

    const copySelection = () => {
      if (disposed || effectiveKindRef.current !== "agy") return;
      const selection = term.getSelection();
      if (!selection) return;
      const count = [...selection.replace(/\r\n/g, "\n")].length;
      void navigator.clipboard
        .writeText(selection)
        .then(() => {
          if (disposed) return;
          showCopyNotice(count === 1 ? "Copied 1 char" : `Copied ${count} chars`);
        })
        .catch(() => {});
    };

    const onContainerMouseDown = (ev: MouseEvent) => {
      if (ev.button !== 0) return;
      mouseDragging = true;
      selectionChangedDuringDrag = false;
      copiedOnMouseUp = false;
    };

    const onWindowMouseUpCapture = (ev: MouseEvent) => {
      if (ev.button !== 0 || !mouseDragging) return;
      mouseDragging = false;
    };

    const onWindowMouseUpBubble = (ev: MouseEvent) => {
      if (ev.button !== 0) return;
      if (selectionChangedDuringDrag && !copiedOnMouseUp) {
        copySelection();
      }
      selectionChangedDuringDrag = false;
      copiedOnMouseUp = false;
    };

    const onWindowBlur = () => {
      mouseDragging = false;
      selectionChangedDuringDrag = false;
      copiedOnMouseUp = false;
    };

    container.addEventListener("mousedown", onContainerMouseDown, true);
    window.addEventListener("mouseup", onWindowMouseUpCapture, true);
    window.addEventListener("mouseup", onWindowMouseUpBubble);
    window.addEventListener("blur", onWindowBlur);

    const selectionDisposable = term.onSelectionChange(() => {
      if (effectiveKindRef.current !== "agy") return;
      if (mouseDragging) {
        selectionChangedDuringDrag = true;
        return;
      }
      if (!term.getSelection()) return;
      copiedOnMouseUp = true;
      copySelection();
    });

    // Subscribe to live ConPTY output FIRST, then hydrate scrollback, merging both by stream
    // offset. Coalesce high-frequency streaming bursts within a single animation frame so
    // heavy output (diffs, builds) doesn't thrash xterm's parser/WebGL uploader.
    let hydrated = false;
    let streamGen = -1;
    let writtenEnd = 0;
    const pending: PtyOutputEvent[] = [];
    const writeQueue: Uint8Array[] = [];
    let flushRaf = 0;
    let lastImmediateWrite = 0;

    const flushWrites = () => {
      flushRaf = 0;
      if (disposed || writeQueue.length === 0) {
        writeQueue.length = 0;
        return;
      }
      const batch = concatBytes(writeQueue.splice(0));
      term.write(batch);
    };

    const enqueueBytes = (bytes: Uint8Array) => {
      const now = performance.now();
      // First chunk after a brief quiet gap writes immediately (0ms typing echo latency).
      if (flushRaf === 0 && writeQueue.length === 0 && now - lastImmediateWrite > 14) {
        lastImmediateWrite = now;
        term.write(bytes);
        return;
      }
      writeQueue.push(bytes);
      if (flushRaf === 0) {
        flushRaf = requestAnimationFrame(flushWrites);
      }
    };

    const writeChunk = (ev: PtyOutputEvent) => {
      if (ev.generation < streamGen) return; // late chunk from a previous, killed PTY
      let bytes = decodeBase64ToBytes(ev.dataB64);
      if (ev.generation !== streamGen) {
        // The PTY was respawned under this session id: a new stream starts at offset 0.
        streamGen = ev.generation;
        writtenEnd = 0;
      }
      if (ev.endOffset <= writtenEnd) return; // already contained in the scrollback snapshot
      const start = ev.endOffset - bytes.length;
      if (start < writtenEnd) bytes = bytes.subarray(writtenEnd - start);
      writtenEnd = ev.endOffset;
      enqueueBytes(bytes);
    };

    void (async () => {
      try {
        const unlisten = await veloxApi.subscribePtyOutput(session.id, (ev) => {
          if (disposed) return;
          if (hydrated) writeChunk(ev);
          else pending.push(ev);
        });
        if (disposed) {
          unlisten();
          return;
        }
        unlistenOutput = unlisten;
      } catch {
        // Ignore outside Tauri
      }

      try {
        const snap = await veloxApi.getScrollback(session.id);
        if (!disposed && snap) {
          streamGen = snap.generation;
          writtenEnd = snap.endOffset;
          if (snap.dataB64) {
            term.write(decodeBase64ToBytes(snap.dataB64), () => {
              if (!disposed) fitIfVisible();
            });
          }
        }
      } catch {
        // Ignore scrollback fetch error
      }

      if (disposed) return;
      hydrated = true;
      for (const ev of pending.splice(0)) writeChunk(ev);
      // Make sure the agent sees the real pane size once it is up.
      fitIfVisible();
    })();

    const observer = new ResizeObserver(() => {
      if (resizeTimerRef.current !== null) {
        window.clearTimeout(resizeTimerRef.current);
      }
      resizeTimerRef.current = window.setTimeout(() => fitIfVisible(), 45);
    });
    observer.observe(container);

    return () => {
      disposed = true;
      if (flushRaf) cancelAnimationFrame(flushRaf);
      writeQueue.length = 0;
      observer.disconnect();
      if (resizeTimerRef.current !== null) {
        window.clearTimeout(resizeTimerRef.current);
      }
      unlistenOutput?.();
      container.removeEventListener("mousedown", onContainerMouseDown, true);
      window.removeEventListener("mouseup", onWindowMouseUpCapture, true);
      window.removeEventListener("mouseup", onWindowMouseUpBubble);
      window.removeEventListener("blur", onWindowBlur);
      selectionDisposable.dispose();
      dataDisposable.dispose();
      window.clearTimeout(copyNoticeTimerRef.current);
      setCopyNotice(null);
      webglRef.current?.dispose();
      webglRef.current = null;
      fitAddon.dispose();
      term.dispose();
      termRef.current = null;
      fitAddonRef.current = null;
    };
    // Intentionally mount/unmount strictly on session identity or sleep state change
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session.id, isSleeping]);

  // GPU renderer only for the visible pane: each WebGL context holds a texture atlas in the
  // GPU process. A pane hidden for a while drops it (xterm falls back to its DOM renderer,
  // which costs nothing while display:none) and gets it back when shown again.
  useEffect(() => {
    if (isSleeping) return;
    const term = termRef.current;
    if (!term) return;

    if (isActive) {
      if (!webglRef.current) {
        try {
          const addon = new WebglAddon();
          addon.onContextLoss(() => {
            addon.dispose();
            if (webglRef.current === addon) webglRef.current = null;
          });
          term.loadAddon(addon);
          fixTransparentCellBackgrounds(addon);
          webglRef.current = addon;
        } catch {
          webglRef.current = null;
        }
      }
      return;
    }

    const timer = window.setTimeout(() => {
      webglRef.current?.dispose();
      webglRef.current = null;
    }, WEBGL_RELEASE_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [isActive, isSleeping, session.id]);

  // The PTY of a new or woken tab is spawned in the background: once it exists (pid set),
  // push the pane's real size to it.
  useEffect(() => {
    if (session.pid != null) fitIfVisible(true);
  }, [session.pid, fitIfVisible]);

  // Live theme & transparency updates (activeAgent change or animated toggle) without remounting
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.allowTransparency = theme.id !== "default";
    term.options.theme = theme.xterm;
    term.refresh(0, term.rows - 1);
  }, [theme]);

  // Live font preference updates without tearing down PTY/terminal
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.fontFamily = fontFamily;
    term.options.fontSize = fontSize;
    fitIfVisible();
  }, [fontFamily, fontSize, fitIfVisible]);

  // Sync viewport & refit before paint when tab becomes active, and focus synchronously
  useLayoutEffect(() => {
    if (!isActive || isSleeping || !termRef.current) return;
    fitIfVisible();
    termRef.current.focus();
  }, [isActive, isSleeping, fitIfVisible]);

  const handleContextMenu = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      e.preventDefault();
      const term = termRef.current;
      if (!term) return;
      const selection = term.getSelection();
      if (selection) {
        void navigator.clipboard.writeText(selection).catch(() => {});
        term.clearSelection();
        showCopyNotice("Copied to clipboard");
      } else {
        void navigator.clipboard
          .readText()
          .then((text) => {
            if (text) term.paste(text);
          })
          .catch(() => {});
      }
    },
    [showCopyNotice],
  );

  if (isSleeping) {
    if (!isActive) return null;

    return (
      <div
        ref={sleepRootRef}
        className="relative z-10 h-full w-full bg-[var(--velox-canvas)] flex items-center justify-center p-6 outline-none velox-chrome-transition"
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            handleWake();
          }
        }}
      >
        {/* Soft ambient radial wash behind sleeping card */}
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 opacity-60"
          style={{
            background:
              "radial-gradient(circle 420px at 50% 45%, color-mix(in srgb, var(--velox-accent) 12%, transparent), transparent 70%)",
          }}
        />

        <div className="relative w-full max-w-[400px] rounded-2xl bg-[var(--velox-surface)]/95 border border-white/[0.08] p-7 text-center shadow-[0_24px_64px_-20px_rgba(0,0,0,0.75)] animate-velox-pop-in">
          <div className="relative w-13 h-13 mx-auto rounded-2xl bg-[var(--velox-canvas)] border border-white/[0.08] flex items-center justify-center shadow-inner">
            <AgentIcon kind={effectiveKind} className="w-6 h-6" active={false} />
            <span className="absolute -bottom-1 -right-1 w-5 h-5 rounded-full bg-[var(--velox-elevated)] border border-white/[0.1] flex items-center justify-center">
              <Moon className="w-2.5 h-2.5 text-[var(--velox-accent-soft)]" />
            </span>
          </div>

          <h3 className="mt-4 text-[16.5px] font-medium text-[var(--velox-ink)] truncate">
            {session.title}
          </h3>
          <p
            title={session.cwd}
            className="mt-1 inline-flex items-center gap-1.5 text-[12px] font-mono text-[var(--velox-ink-soft)] max-w-full"
          >
            <Folder className="w-3 h-3 shrink-0 text-[var(--velox-ink-muted)]" />
            <span className="truncate">{getCwdBasename(session.cwd)}</span>
          </p>

          <div className="mt-3.5 inline-flex items-center gap-1.5 px-3 py-1 rounded-full bg-[var(--velox-elevated)] border border-white/[0.07] text-[11.5px] font-mono tabular-nums text-[var(--velox-ink-secondary)]">
            <Sparkles className="w-3 h-3 text-[var(--velox-accent-soft)]" />
            <span>Hibernated · 0 MB RAM</span>
          </div>

          <button
            type="button"
            onClick={handleWake}
            className="mt-5 w-full h-10 px-4 rounded-xl bg-[var(--velox-accent)]/18 hover:bg-[var(--velox-accent)]/28 active:scale-[0.985] text-[var(--velox-accent-soft)] border border-[var(--velox-accent)]/35 hover:border-[var(--velox-accent)]/55 text-[13px] font-medium inline-flex items-center justify-center gap-2 transition-all duration-150 cursor-pointer shadow-[0_8px_24px_-12px_var(--velox-accent)]"
          >
            <Play className="w-3.5 h-3.5 fill-current" />
            <span>Wake session</span>
          </button>

          <p className="mt-3 text-[11.5px] text-[var(--velox-ink-muted)]">
            {session.agentSessionId
              ? "Resumes previous conversation automatically · Press Enter"
              : "Press Enter to launch agent in this folder"}
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className={isActive ? "relative z-10 h-full w-full overflow-hidden" : "hidden"}>
      <div
        ref={containerRef}
        onContextMenu={handleContextMenu}
        className="h-full w-full"
        style={
          {
            "--velox-term-bg": theme.createScene ? "transparent" : theme.background,
          } as React.CSSProperties
        }
      />
      {paneNotice && (
        <div
          role="status"
          className="pointer-events-none absolute right-3.5 top-3 max-w-[80%] px-3 py-1.5 rounded-lg bg-[var(--velox-elevated)]/95 border border-white/[0.12] text-[12px] text-[var(--velox-ink)] shadow-xl animate-velox-toast-in"
        >
          {paneNotice}
        </div>
      )}
    </div>
  );
});
