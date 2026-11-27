import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import {
  AlertCircle,
  ArrowLeft,
  Eye,
  Loader2,
  Moon,
  RefreshCw,
  Send,
  ShieldAlert,
  WifiOff,
} from "lucide-react";
import { decodeBase64ToBytes, veloxApi } from "../../lib/tauri";
import { getAgentTheme } from "../../themes/registry";
import type {
  RemoteSessionInfo,
  TaskTicket,
  TeamPtyOutputEvent,
} from "../../types/ipc";
import { AgentIcon } from "../icons/AgentIcons";
import { getSessionStatusDotClass } from "./taskStyles";

export interface RemoteTerminalPaneProps {
  peer: string;
  sessionId: string;
  remoteSession: RemoteSessionInfo | null;
  peerOnline: boolean;
  /** True only when mutual "Control total" is active with this peer. */
  fullControlActive: boolean;
  tasks: TaskTicket[];
  fontFamily: string;
  fontSize: number;
  onSendTask: (
    peer: string | null,
    sessionId: string,
    text: string,
  ) => Promise<TaskTicket>;
  onBack: () => void;
}

interface XtermRenderDimensions {
  _core?: {
    _renderService?: {
      dimensions?: {
        css?: {
          canvas?: { width: number; height: number };
          cell?: { width: number; height: number };
        };
      };
    };
  };
}

const TERM_PAD_X = 24;
const TERM_PAD_Y = 16;

function measureTerminalPixels(
  term: Terminal,
  cols: number,
  rows: number,
  fontSize: number,
): { w: number; h: number; screenW: number; screenH: number } {
  const css = (term as unknown as XtermRenderDimensions)._core?._renderService
    ?.dimensions?.css;
  const rawCanvasW = css?.canvas?.width ?? 0;
  const rawCanvasH = css?.canvas?.height ?? 0;
  const cellW = css?.cell?.width ?? 0;
  const cellH = css?.cell?.height ?? 0;

  const screenW =
    rawCanvasW > 0
      ? Math.ceil(rawCanvasW)
      : cellW > 0
        ? Math.ceil(cellW * cols)
        : Math.ceil(cols * fontSize * 0.62);
  const screenH =
    rawCanvasH > 0
      ? Math.ceil(rawCanvasH)
      : cellH > 0
        ? Math.ceil(cellH * rows)
        : Math.ceil(rows * fontSize * 1.24);

  return {
    screenW: Math.max(1, screenW),
    screenH: Math.max(1, screenH),
    w: Math.max(1, screenW + TERM_PAD_X),
    h: Math.max(1, screenH + TERM_PAD_Y),
  };
}

function formatTicketStatus(
  ticket: TaskTicket,
  peer: string,
): { text: string; toneClass: string } {
  switch (ticket.state) {
    case "pending":
      return {
        text: `Waiting for ${peer} to accept…`,
        toneClass: "text-amber-300",
      };
    case "accepted":
      return {
        text: "Accepted — delivering…",
        toneClass: "text-sky-300",
      };
    case "delivered":
      return {
        text: "Delivered",
        toneClass: "text-emerald-300",
      };
    case "rejected":
      return {
        text: ticket.message || "Rejected",
        toneClass: "text-rose-300",
      };
    case "failed":
      return {
        text: ticket.message || "Delivery failed",
        toneClass: "text-rose-300",
      };
    case "expired":
      return {
        text: ticket.message || "Request expired",
        toneClass: "text-[var(--velox-ink-muted)]",
      };
  }
}

export function RemoteTerminalPane({
  peer,
  sessionId,
  remoteSession,
  peerOnline,
  fullControlActive,
  tasks,
  fontFamily,
  fontSize,
  onSendTask,
  onBack,
}: RemoteTerminalPaneProps) {
  const [cachedMeta, setCachedMeta] = useState<RemoteSessionInfo | null>(
    remoteSession,
  );
  useEffect(() => {
    setCachedMeta(remoteSession);
    // Reset cached metadata only when target changes; keep last known metadata if peer goes offline
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [peer, sessionId]);
  useEffect(() => {
    if (remoteSession) setCachedMeta(remoteSession);
  }, [remoteSession]);

  const meta = remoteSession ?? cachedMeta;
  const initialCols = meta?.cols ?? 120;
  const initialRows = meta?.rows ?? 34;

  const measureRef = useRef<HTMLDivElement | null>(null);
  const hostRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const webglRef = useRef<WebglAddon | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  const [connecting, setConnecting] = useState(true);
  const [subscribeError, setSubscribeError] = useState<string | null>(null);
  const [sessionGone, setSessionGone] = useState(false);
  const [retryNonce, setRetryNonce] = useState(0);

  const [gridSize, setGridSize] = useState<{ cols: number; rows: number }>({
    cols: initialCols,
    rows: initialRows,
  });
  const [availSize, setAvailSize] = useState<{ w: number; h: number }>({
    w: 0,
    h: 0,
  });
  const [termPixels, setTermPixels] = useState<{
    w: number;
    h: number;
    screenW: number;
    screenH: number;
  }>({
    w: Math.ceil(initialCols * fontSize * 0.62) + TERM_PAD_X,
    h: Math.ceil(initialRows * fontSize * 1.24) + TERM_PAD_Y,
    screenW: Math.ceil(initialCols * fontSize * 0.62),
    screenH: Math.ceil(initialRows * fontSize * 1.24),
  });

  const [taskText, setTaskText] = useState("");
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);
  const [inputError, setInputError] = useState<string | null>(null);

  const canControl = fullControlActive && peerOnline && !sessionGone;
  const canControlRef = useRef(canControl);
  canControlRef.current = canControl;

  const effectiveKind = meta?.activeAgent ?? meta?.agentKind ?? "shell";
  const agentTheme = useMemo(
    () => getAgentTheme(effectiveKind, false),
    [effectiveKind],
  );

  const recomputeTerminalPixels = useCallback(
    (cols: number, rows: number) => {
      const term = termRef.current;
      if (!term) return;
      setTermPixels(measureTerminalPixels(term, cols, rows, fontSize));
    },
    [fontSize],
  );

  // Track available viewport space for scaling
  useEffect(() => {
    const el = measureRef.current;
    if (!el) return;

    const updateAvail = () => {
      setAvailSize({ w: el.clientWidth, h: el.clientHeight });
    };

    updateAvail();
    const observer = new ResizeObserver(updateAvail);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // Recompute pixel size whenever grid cols/rows or font options change
  useLayoutEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.fontFamily = fontFamily;
    term.options.fontSize = fontSize;
    recomputeTerminalPixels(gridSize.cols, gridSize.rows);
    const raf = window.requestAnimationFrame(() => {
      recomputeTerminalPixels(gridSize.cols, gridSize.rows);
    });
    return () => window.cancelAnimationFrame(raf);
  }, [fontFamily, fontSize, gridSize.cols, gridSize.rows, recomputeTerminalPixels]);

  // Update theme live without remounting
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = agentTheme.xterm;
    term.refresh(0, Math.max(0, term.rows - 1));
  }, [agentTheme]);

  // Toggle interactive keyboard control instantaneously without remounting xterm
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.disableStdin = !canControl;
    term.options.cursorBlink = canControl;
    if (!canControl) {
      setInputError(null);
    }
  }, [canControl]);

  // Mount xterm (interactive when mutual Control total is active, read-only otherwise),
  // subscribe to remote stream, unsubscribe on unmount/change
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    let disposed = false;
    let unlistenSnapshot: (() => void) | undefined;
    let unlistenOutput: (() => void) | undefined;
    let unlistenResize: (() => void) | undefined;
    let unlistenGone: (() => void) | undefined;

    setConnecting(true);
    setSubscribeError(null);
    setSessionGone(false);
    setInputError(null);
    setGridSize({ cols: initialCols, rows: initialRows });

    const term = new Terminal({
      disableStdin: !canControlRef.current,
      theme: agentTheme.xterm,
      allowTransparency: false,
      cursorBlink: canControlRef.current,
      cursorStyle: "bar",
      scrollback: 5000,
      allowProposedApi: true,
      fontFamily,
      fontSize,
      lineHeight: 1.24,
      letterSpacing: 0,
      tabStopWidth: 4,
      cols: initialCols,
      rows: initialRows,
    });

    const unicode11 = new Unicode11Addon();
    term.loadAddon(new WebLinksAddon());
    term.loadAddon(unicode11);
    term.unicode.activeVersion = "11";

    term.attachCustomKeyEventHandler((ev: KeyboardEvent) => {
      if (ev.type !== "keydown") return true;
      const key = ev.key.toLowerCase();

      if (ev.ctrlKey && ev.shiftKey && !ev.altKey && key === "c") {
        const selection = term.getSelection();
        if (selection) {
          void navigator.clipboard.writeText(selection).catch(() => {});
        }
        return false;
      }

      if (ev.ctrlKey && !ev.shiftKey && !ev.altKey && key === "c") {
        const selection = term.getSelection();
        if (selection) {
          void navigator.clipboard.writeText(selection).catch(() => {});
          term.clearSelection();
          return false;
        }
        // Without a selection: send SIGINT (\x03) only when Control total is active
        return canControlRef.current;
      }

      if (!canControlRef.current) {
        return false;
      }

      // Let browser fire native paste (xterm sends pasted text to onData)
      if (ev.ctrlKey && !ev.altKey && key === "v") {
        return false;
      }

      // Preserve Velox global shortcuts (handled by App window listener)
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

    const dataDisposable = term.onData((data) => {
      if (!canControlRef.current) return;
      void veloxApi.sendTeamInput(peer, sessionId, data).catch((err) => {
        if (!disposed) {
          setInputError(
            err instanceof Error && err.message
              ? err.message
              : "No se pudo enviar entrada remota",
          );
        }
      });
    });

    term.open(host);
    termRef.current = term;

    try {
      const addon = new WebglAddon();
      addon.onContextLoss(() => {
        addon.dispose();
        if (webglRef.current === addon) webglRef.current = null;
      });
      term.loadAddon(addon);
      webglRef.current = addon;
    } catch {
      webglRef.current = null;
    }

    setTermPixels(
      measureTerminalPixels(term, initialCols, initialRows, fontSize),
    );

    let hasSnapshot = false;
    let streamGen = -1;
    let writtenEnd = 0;
    const pending: TeamPtyOutputEvent[] = [];

    const applyOutputChunk = (ev: TeamPtyOutputEvent) => {
      if (ev.generation < streamGen) return;
      if (ev.generation !== streamGen) {
        streamGen = ev.generation;
        writtenEnd = 0;
      }
      if (ev.endOffset <= writtenEnd) return;
      let bytes = decodeBase64ToBytes(ev.dataB64);
      const start = ev.endOffset - bytes.length;
      if (start < writtenEnd) {
        bytes = bytes.subarray(writtenEnd - start);
      }
      writtenEnd = ev.endOffset;
      term.write(bytes);
    };

    void (async () => {
      try {
        const [uSnap, uOut, uResize, uGone] = await Promise.all([
          veloxApi.onTeamPtySnapshot((ev) => {
            if (disposed || ev.peer !== peer || ev.sessionId !== sessionId) {
              return;
            }
            hasSnapshot = true;
            streamGen = ev.generation;
            writtenEnd = ev.endOffset;
            term.reset();
            term.resize(ev.cols, ev.rows);
            setGridSize({ cols: ev.cols, rows: ev.rows });
            setTermPixels(
              measureTerminalPixels(term, ev.cols, ev.rows, fontSize),
            );
            if (ev.dataB64) {
              term.write(decodeBase64ToBytes(ev.dataB64));
            }
            for (const queued of pending.splice(0)) {
              applyOutputChunk(queued);
            }
            setConnecting(false);
            setSubscribeError(null);
          }),
          veloxApi.onTeamPtyOutput((ev) => {
            if (disposed || ev.peer !== peer || ev.sessionId !== sessionId) {
              return;
            }
            if (!hasSnapshot) {
              pending.push(ev);
              return;
            }
            applyOutputChunk(ev);
          }),
          veloxApi.onTeamPtyResize((ev) => {
            if (disposed || ev.peer !== peer || ev.sessionId !== sessionId) {
              return;
            }
            term.resize(ev.cols, ev.rows);
            setGridSize({ cols: ev.cols, rows: ev.rows });
            setTermPixels(
              measureTerminalPixels(term, ev.cols, ev.rows, fontSize),
            );
          }),
          veloxApi.onTeamSessionGone((ev) => {
            if (disposed || ev.peer !== peer || ev.sessionId !== sessionId) {
              return;
            }
            setSessionGone(true);
            setConnecting(false);
          }),
        ]);

        if (disposed) {
          uSnap();
          uOut();
          uResize();
          uGone();
          return;
        }
        unlistenSnapshot = uSnap;
        unlistenOutput = uOut;
        unlistenResize = uResize;
        unlistenGone = uGone;
      } catch (err) {
        if (!disposed) {
          setSubscribeError(
            err instanceof Error
              ? err.message
              : "Failed to listen to remote terminal events",
          );
          setConnecting(false);
        }
        return;
      }

      try {
        await veloxApi.teamSubscribe(peer, sessionId);
      } catch (err) {
        if (!disposed) {
          setSubscribeError(
            err instanceof Error
              ? err.message
              : `Could not connect to ${peer}`,
          );
          setConnecting(false);
        }
      }
    })();

    return () => {
      disposed = true;
      unlistenSnapshot?.();
      unlistenOutput?.();
      unlistenResize?.();
      unlistenGone?.();
      dataDisposable.dispose();
      void veloxApi.teamUnsubscribe(peer, sessionId).catch(() => {});
      webglRef.current?.dispose();
      webglRef.current = null;
      term.dispose();
      termRef.current = null;
    };
    // Mount/subscribe lifecycle is tied strictly to target (peer, sessionId) and explicit retry
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [peer, sessionId, retryNonce]);

  // Reset draft & error when switching target
  useEffect(() => {
    setTaskText("");
    setSendError(null);
    if (textareaRef.current) {
      textareaRef.current.style.height = "48px";
    }
  }, [peer, sessionId]);

  const handleContextMenu = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      e.preventDefault();
      const term = termRef.current;
      if (!term) return;
      const selection = term.getSelection();
      if (selection) {
        void navigator.clipboard.writeText(selection).catch(() => {});
        term.clearSelection();
      } else if (canControlRef.current) {
        void navigator.clipboard
          .readText()
          .then((text) => {
            if (text && canControlRef.current) term.paste(text);
          })
          .catch(() => {});
      }
    },
    [],
  );

  const adjustTextareaHeight = (el: HTMLTextAreaElement) => {
    el.style.height = "auto";
    const clamped = Math.min(136, Math.max(48, el.scrollHeight));
    el.style.height = `${clamped}px`;
  };

  const handleSend = useCallback(async () => {
    const trimmed = taskText.trim();
    if (!trimmed || sending || !peerOnline || sessionGone) return;
    setSending(true);
    setSendError(null);
    try {
      await onSendTask(peer, sessionId, trimmed);
      setTaskText("");
      if (textareaRef.current) {
        textareaRef.current.style.height = "48px";
      }
    } catch (err) {
      setSendError(
        err instanceof Error && err.message
          ? err.message
          : "Failed to send task",
      );
    } finally {
      setSending(false);
    }
  }, [taskText, sending, peerOnline, sessionGone, onSendTask, peer, sessionId]);

  const newestTicket = useMemo(
    () =>
      tasks.find(
        (t) =>
          t.direction === "outgoing" &&
          t.peer === peer &&
          t.sessionId === sessionId,
      ) ?? null,
    [tasks, peer, sessionId],
  );

  const ticketStatus = newestTicket
    ? formatTicketStatus(newestTicket, peer)
    : null;

  const scale =
    availSize.w > 0 && availSize.h > 0
      ? Math.min(1, availSize.w / termPixels.w, availSize.h / termPixels.h)
      : 1;
  const scaledW = Math.max(1, Math.floor(termPixels.w * scale));
  const scaledH = Math.max(1, Math.floor(termPixels.h * scale));

  const displayTitle = meta?.title ?? sessionId;
  const statusLabel = !peerOnline
    ? "offline"
    : sessionGone
      ? "ended"
      : (meta?.status ?? "running");

  return (
    <div className="relative z-10 h-full w-full flex flex-col bg-[var(--velox-canvas)] overflow-hidden">
      {/* Header strip */}
      <div className="h-9 shrink-0 px-3 border-b border-white/[0.07] bg-[var(--velox-surface)]/80 flex items-center justify-between gap-3">
        <div className="flex items-center gap-2 min-w-0">
          <button
            type="button"
            onClick={onBack}
            title="Back to local sessions"
            aria-label="Back to local sessions"
            className="h-6 w-6 flex items-center justify-center rounded text-[var(--velox-ink-muted)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] transition-colors cursor-pointer shrink-0"
          >
            <ArrowLeft className="w-3.5 h-3.5" />
          </button>

          <span
            title={peerOnline && !sessionGone ? "Live" : statusLabel}
            className={`w-2 h-2 rounded-full shrink-0 ${
              sessionGone
                ? "bg-rose-400"
                : peerOnline
                  ? "bg-emerald-400"
                  : "bg-[var(--velox-ink-faint)]"
            }`}
          />

          <AgentIcon
            kind={effectiveKind}
            className="w-4 h-4 shrink-0"
            active={peerOnline && !sessionGone}
            blocked={meta?.status === "blocked"}
          />

          <div className="flex items-center gap-1.5 min-w-0 text-[12.5px] font-medium">
            <span className="shrink-0" style={{ color: "#F5B971" }}>
              {peer}
            </span>
            <span className="text-[var(--velox-ink-muted)] shrink-0">·</span>
            <span className="text-[var(--velox-ink)] truncate">
              {displayTitle}
            </span>
          </div>

          <span className="shrink-0 inline-flex items-center gap-1 px-1.5 py-0.5 rounded bg-white/[0.04] border border-white/[0.06] text-[11px] text-[var(--velox-ink-secondary)] capitalize">
            <span
              className={`w-1.5 h-1.5 rounded-full ${
                !peerOnline || sessionGone
                  ? "bg-[var(--velox-ink-faint)]"
                  : getSessionStatusDotClass(meta?.status ?? "running")
              }`}
            />
            <span>{statusLabel}</span>
          </span>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          <span className="text-[11px] font-mono tabular-nums text-[var(--velox-ink-muted)] hidden sm:inline">
            {gridSize.cols}×{gridSize.rows}
          </span>
          {canControl ? (
            <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded bg-amber-500/18 border border-amber-400/40 text-[11px] font-medium text-amber-200">
              <ShieldAlert className="w-3 h-3 text-amber-300" />
              <span>Control total</span>
            </span>
          ) : (
            <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded bg-white/[0.05] border border-white/[0.08] text-[11px] text-[var(--velox-ink-secondary)]">
              <Eye className="w-3 h-3 text-[var(--velox-ink-muted)]" />
              <span>Read-only</span>
            </span>
          )}
        </div>
      </div>

      {/* Scaled terminal mirror viewport */}
      <div className="flex-1 min-h-0 p-2 relative overflow-hidden flex flex-col">
        <div
          ref={measureRef}
          className="flex-1 min-h-0 w-full relative flex items-center justify-center overflow-hidden"
        >
          <div
            style={{
              width: `${scaledW}px`,
              height: `${scaledH}px`,
              position: "relative",
            }}
            className="rounded-lg overflow-hidden border border-white/[0.06] shadow-inner"
          >
            <div
              ref={hostRef}
              onContextMenu={handleContextMenu}
              className="velox-remote-xterm"
              style={
                {
                  width: `${termPixels.w}px`,
                  height: `${termPixels.h}px`,
                  transform: `scale(${scale})`,
                  transformOrigin: "top left",
                  "--velox-term-bg": agentTheme.background,
                  "--velox-remote-screen-w": `${termPixels.screenW}px`,
                  "--velox-remote-screen-h": `${termPixels.screenH}px`,
                } as React.CSSProperties
              }
            />
          </div>

          {/* Loading overlay */}
          {connecting && !subscribeError && !sessionGone && (
            <div className="absolute inset-0 flex items-center justify-center bg-[var(--velox-canvas)]/85 backdrop-blur-[1px]">
              <div className="flex items-center gap-2.5 px-4 py-2.5 rounded-xl bg-[var(--velox-elevated)] border border-white/[0.08] text-[12.5px] text-[var(--velox-ink)] shadow-xl">
                <Loader2 className="w-4 h-4 animate-spin text-[#9CC2FF]" />
                <span>Connecting to {peer}…</span>
              </div>
            </div>
          )}

          {/* Subscribe error overlay */}
          {subscribeError && !sessionGone && (
            <div className="absolute inset-0 flex items-center justify-center bg-[var(--velox-canvas)]/85 p-6">
              <div className="w-full max-w-[360px] rounded-xl bg-[var(--velox-elevated)] border border-rose-500/25 p-5 text-center shadow-xl space-y-3">
                <AlertCircle className="w-6 h-6 text-rose-400 mx-auto" />
                <div className="space-y-1">
                  <div className="text-[13px] font-medium text-[var(--velox-ink)]">
                    Unable to mirror session
                  </div>
                  <div className="text-[12px] text-rose-300 break-words">
                    {subscribeError}
                  </div>
                </div>
                <div className="flex items-center justify-center gap-2 pt-1">
                  <button
                    type="button"
                    onClick={onBack}
                    className="h-8 px-3 rounded-lg bg-[#1C2130] hover:bg-[#252C40] border border-white/[0.08] text-[12px] text-[var(--velox-ink-secondary)] cursor-pointer"
                  >
                    Go back
                  </button>
                  <button
                    type="button"
                    onClick={() => setRetryNonce((n) => n + 1)}
                    className="h-8 px-3 rounded-lg bg-[#4E8DF5]/15 hover:bg-[#4E8DF5]/25 border border-[#4E8DF5]/30 text-[12px] font-medium text-[#9CC2FF] inline-flex items-center gap-1.5 cursor-pointer"
                  >
                    <RefreshCw className="w-3.5 h-3.5" />
                    <span>Retry</span>
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* Sleeping shared session hint (under Control total, sending a task or typing wakes it) */}
          {meta?.status === "sleeping" && !connecting && !subscribeError && !sessionGone && (
            <div className="pointer-events-none absolute top-3 left-1/2 -translate-x-1/2 px-3.5 py-1.5 rounded-full bg-[var(--velox-elevated)]/95 border border-white/[0.1] text-[11.5px] text-[var(--velox-ink-secondary)] inline-flex items-center gap-2 shadow-lg">
              <Moon className="w-3.5 h-3.5 text-amber-300 shrink-0" />
              <span>
                Sesión dormida · envía una tarea o escribe en la terminal para despertarla
              </span>
            </div>
          )}

          {/* Session gone overlay */}
          {sessionGone && (
            <div className="absolute inset-0 flex items-center justify-center bg-[var(--velox-canvas)]/88 backdrop-blur-[1px] p-6">
              <div className="w-full max-w-[340px] rounded-xl bg-[var(--velox-elevated)] border border-white/[0.09] p-5 text-center shadow-xl space-y-3">
                <div className="text-[13.5px] font-medium text-[var(--velox-ink)]">
                  Session no longer shared
                </div>
                <p className="text-[12px] text-[var(--velox-ink-muted)]">
                  {peer} unshared, slept, or closed this session.
                </p>
                <button
                  type="button"
                  onClick={onBack}
                  className="h-8 px-4 rounded-lg bg-[#4E8DF5] hover:bg-[#5E98F7] text-[#080B12] text-[12px] font-semibold cursor-pointer"
                >
                  Go back
                </button>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Send a task bar */}
      <div className="shrink-0 border-t border-white/[0.07] bg-[var(--velox-surface)]/90 px-3.5 py-2.5 space-y-1.5">
        {!peerOnline && !sessionGone && (
          <div className="flex items-center gap-1.5 text-[11.5px] text-amber-300/90">
            <WifiOff className="w-3.5 h-3.5 shrink-0" />
            <span>{peer} is offline. Task sending is disabled until reconnected.</span>
          </div>
        )}

        <div className="flex items-end gap-2">
          <textarea
            ref={textareaRef}
            rows={2}
            value={taskText}
            disabled={!peerOnline || sessionGone || sending}
            placeholder={
              peerOnline
                ? `Send a task to ${peer} · ${displayTitle}…`
                : `${peer} is offline`
            }
            onChange={(e) => {
              setTaskText(e.target.value);
              adjustTextareaHeight(e.target);
            }}
            onKeyDown={(e) => {
              if (e.ctrlKey && e.key === "Enter") {
                e.preventDefault();
                e.stopPropagation();
                void handleSend();
              }
            }}
            className="flex-1 min-h-[48px] max-h-[136px] resize-none rounded-lg bg-[var(--velox-canvas)] border border-white/[0.08] px-3 py-2 text-[12.5px] font-mono text-[var(--velox-ink)] placeholder:text-[var(--velox-ink-muted)] outline-none focus:border-[#4E8DF5]/60 disabled:opacity-50 disabled:cursor-not-allowed"
          />
          <button
            type="button"
            disabled={
              !peerOnline || sessionGone || sending || taskText.trim().length === 0
            }
            onClick={() => void handleSend()}
            title="Send task (Ctrl+Enter)"
            className="h-9 px-3.5 rounded-lg bg-[#4E8DF5] hover:bg-[#5E98F7] disabled:opacity-45 disabled:cursor-not-allowed text-[#080B12] text-[12.5px] font-semibold inline-flex items-center gap-1.5 transition-colors cursor-pointer shrink-0"
          >
            {sending ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <Send className="w-3.5 h-3.5" />
            )}
            <span>Send task</span>
          </button>
        </div>

        <div className="flex items-center justify-between gap-3 text-[11px]">
          <span className="text-[var(--velox-ink-muted)]">
            {canControl
              ? "Control total activo · las tareas llegan directo y tu teclado controla esta terminal. "
              : "They review it before it runs. "}
            <span className="hidden sm:inline text-[var(--velox-ink-faint)]">
              Ctrl+Enter to send
            </span>
          </span>

          <div className="min-w-0 truncate">
            {sendError || inputError ? (
              <span className="text-rose-300">{sendError ?? inputError}</span>
            ) : ticketStatus ? (
              <span className={ticketStatus.toneClass}>{ticketStatus.text}</span>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
