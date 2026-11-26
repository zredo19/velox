import { useEffect, useRef, useState, type RefObject } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { Terminal } from "@xterm/xterm";
import { encodeBytesToBase64, veloxApi } from "./tauri";

const NOTICE_MS = 6000;

/**
 * Images and dropped files for one terminal pane. Plain text is left to xterm, which
 * already handles the browser paste event (including bracketed paste).
 * Returns a short status message to show over the pane, or null.
 */
export function useTerminalPaste(
  sessionId: string,
  isActive: boolean,
  isMounted: boolean,
  containerRef: RefObject<HTMLDivElement | null>,
  termRef: RefObject<Terminal | null>,
): string | null {
  const [notice, setNotice] = useState<string | null>(null);
  const noticeTimer = useRef<number | undefined>(undefined);
  const runRef = useRef<(task: Promise<string>) => void>(() => {});

  runRef.current = (task) => {
    window.clearTimeout(noticeTimer.current);
    // Only worth showing when it takes a while, e.g. an upload over ssh.
    noticeTimer.current = window.setTimeout(() => setNotice("Copying image…"), 300);
    task.then(
      (text) => {
        window.clearTimeout(noticeTimer.current);
        setNotice(null);
        if (text) termRef.current?.paste(text);
      },
      (error: unknown) => {
        window.clearTimeout(noticeTimer.current);
        setNotice(`Paste failed: ${String(error)}`);
        noticeTimer.current = window.setTimeout(() => setNotice(null), NOTICE_MS);
      },
    );
  };

  useEffect(() => () => window.clearTimeout(noticeTimer.current), []);

  // Clipboard images (Ctrl+V, Ctrl+Shift+V, Shift+Insert all raise this event).
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const onPaste = (event: ClipboardEvent) => {
      const data = event.clipboardData;
      if (!data || data.getData("text/plain")) return;
      const image = Array.from(data.files).find((f) => f.type.startsWith("image/"));
      if (!image) return;
      event.preventDefault();
      event.stopPropagation();
      const extension = image.type.slice("image/".length);
      runRef.current(
        image
          .arrayBuffer()
          .then((buffer) =>
            veloxApi.pasteImage(sessionId, extension, encodeBytesToBase64(new Uint8Array(buffer))),
          ),
      );
    };

    // Capture phase: runs before xterm's own listener on its hidden textarea.
    container.addEventListener("paste", onPaste, true);
    return () => container.removeEventListener("paste", onPaste, true);
    // `isMounted`: the container only exists while awake, so re-attach after waking.
  }, [sessionId, containerRef, isMounted]);

  // Files dropped on the window go to the visible terminal.
  useEffect(() => {
    if (!isActive) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    try {
      void getCurrentWebview()
        .onDragDropEvent((event) => {
          if (event.payload.type !== "drop" || event.payload.paths.length === 0) return;
          termRef.current?.focus();
          runRef.current(veloxApi.pasteFiles(sessionId, event.payload.paths));
        })
        .then((fn) => {
          if (disposed) fn();
          else unlisten = fn;
        })
        .catch(() => {});
    } catch {
      // Running outside Tauri
    }
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [isActive, sessionId, termRef]);

  return notice;
}
