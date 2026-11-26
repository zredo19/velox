import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { Moon, Play } from "lucide-react";
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
  onActivity: ReportActivity;
  onWake: (id: string, cols: number, rows: number) => void;
}

/** A hidden pane keeps its WebGL context this long, so quick tab switching stays instant. */
const WEBGL_RELEASE_DELAY_MS = 30_000;

function getCwdBasename(cwd: string): string {
  const parts = cwd.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? cwd;
}

// Memoized: App re-renders on every telemetry tick, and every pane re-rendering with it is
// wasted work. Props are stable (themes are shared objects, callbacks are memoized).
export const TerminalPane = memo(function TerminalPane({
  session,
  isActive,
  fontFamily,
  fontSize,
  theme,
  onActivity,
  onWake,
}: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const resizeTimerRef = useRef<number | null>(null);
  const webglRef = useRef<WebglAddon | null>(null);
  const copyNoticeTimerRef = useRef<number | undefined>(undefined);
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

  // The only way a pane is resized. A hidden pane must be left alone: measuring it yields
  // a few columns, and that size would be pushed to the running process.
  const fitIfVisible = useCallback(() => {
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
      void veloxApi.resizePty(session.id, term.cols, term.rows);
    } catch {
      // Ignore transient resize errors
    }
  }, [session.id]);

  const handleWake = useCallback(() => {
    onWake(session.id, 120, 34);
  }, [session.id, onWake]);

  // Mount xterm.js only while awake; unmount when sleeping to release WebGL + DOM memory
  useEffect(() => {
    if (isSleeping) {
      return;
    }

    const container = containerRef.current;
    if (!container) return;

    let disposed = false;
    let unlistenOutput: (() => void) | undefined;

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
          return false;
        }
        return true;
      }

      // Velox shortcuts: not sent to the PTY, handled by the window listener in App.
      // Plain Ctrl+B/M/H stay with the terminal program (Claude Code uses Ctrl+B; Ctrl+M is
      // Enter and Ctrl+H Backspace). Plain Ctrl+N is Velox's "new session".
      const key = ev.key.toLowerCase();
      if (
        ev.ctrlKey &&
        !ev.altKey &&
        (ev.shiftKey ? ["n", "b", "m", "h"].includes(key) : /^[n,1-9]$/.test(key))
      ) {
        return false;
      }

      return true;
    });

    termRef.current = term;
    fitAddonRef.current = fitAddon;

    fitIfVisible();
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
          window.clearTimeout(copyNoticeTimerRef.current);
          setCopyNotice(
            count === 1 ? "1 carácter copiado" : `${count} caracteres copiados`,
          );
          copyNoticeTimerRef.current = window.setTimeout(() => {
            setCopyNotice(null);
          }, 2000);
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
    // offset. Fetching scrollback before subscribing dropped whatever the agent printed in
    // between: on a brand-new session that is exactly its startup sequence (alternate screen,
    // keyboard modes...), which left the pane looking like a frozen shell.
    let hydrated = false;
    let streamGen = -1;
    let writtenEnd = 0;
    const pending: PtyOutputEvent[] = [];

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
      term.write(bytes);
    };

    void (async () => {
      try {
        const unlisten = await veloxApi.onPtyOutput((ev) => {
          if (disposed || ev.sessionId !== session.id) return;
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
      resizeTimerRef.current = window.setTimeout(fitIfVisible, 40);
    });
    observer.observe(container);

    return () => {
      disposed = true;
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
    if (session.pid != null) fitIfVisible();
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
      } else {
        void navigator.clipboard
          .readText()
          .then((text) => {
            if (text) term.paste(text);
          })
          .catch(() => {});
      }
    },
    [],
  );

  if (isSleeping) {
    if (!isActive) return null;

    return (
      <div
        className="relative z-10 h-full w-full bg-[var(--velox-canvas)] flex items-center justify-center p-6 outline-none"
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            handleWake();
          }
        }}
      >
        <div className="w-full max-w-[380px] rounded-2xl bg-[#11141C] border border-white/[0.07] p-7 text-center shadow-xl">
          <div className="w-12 h-12 mx-auto rounded-xl bg-[#0B0D12] border border-white/[0.06] flex items-center justify-center">
            <AgentIcon kind={effectiveKind} className="w-6 h-6" active={false} />
          </div>

          <h3 className="mt-4 text-[16px] font-medium text-[#E6EAF2] truncate">
            {session.title}
          </h3>
          <p className="mt-1 text-[12px] font-mono text-[#7E889E] truncate">
            {getCwdBasename(session.cwd)}
          </p>

          <div className="mt-3 inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-[#171B26] border border-white/[0.06] text-[11.5px] font-mono tabular-nums text-[#7E889E]">
            <Moon className="w-3 h-3 text-[#64708A]" />
            <span>Sleeping · 0 MB RAM</span>
          </div>

          <button
            type="button"
            onClick={handleWake}
            className="mt-5 w-full h-9 px-4 rounded-lg bg-[#4E8DF5]/15 hover:bg-[#4E8DF5]/25 text-[#9CC2FF] border border-[#4E8DF5]/30 text-[13px] font-medium inline-flex items-center justify-center gap-2 transition-colors cursor-pointer"
          >
            <Play className="w-3.5 h-3.5 fill-current" />
            <span>Wake session</span>
          </button>

          <p className="mt-3 text-[11.5px] text-[#64708A]">
            {session.agentSessionId
              ? "Resumes previous conversation automatically"
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
          className="pointer-events-none absolute right-3 top-3 max-w-[80%] px-3 py-1.5 rounded-lg bg-[var(--velox-elevated)] border border-white/[0.1] text-[12px] text-[var(--velox-ink)] shadow-lg"
        >
          {paneNotice}
        </div>
      )}
    </div>
  );
});
