import { useEffect, useRef, type RefObject } from "react";
import type { Terminal } from "@xterm/xterm";
import type { AgentKind } from "../types/ipc";
import { detectActivity, type AgentActivity } from "./agentActivity";

const POLL_MS = 500;

export type ReportActivity = (sessionId: string, activity: AgentActivity | null) => void;

function readScreen(term: Terminal): string[] {
  const buffer = term.buffer.active;
  return Array.from(
    { length: term.rows },
    (_, row) => buffer.getLine(buffer.baseY + row)?.translateToString(true) ?? "",
  );
}

/**
 * Watches one awake terminal and reports when its agent starts working, asks something or
 * goes idle. Returns the ref the terminal must keep its window title in.
 */
export function useAgentActivity(
  sessionId: string,
  kind: AgentKind,
  isMounted: boolean,
  termRef: RefObject<Terminal | null>,
  report: ReportActivity,
): RefObject<string> {
  const titleRef = useRef("");

  useEffect(() => {
    if (!isMounted) {
      titleRef.current = "";
    }
    return () => {
      titleRef.current = "";
    };
  }, [sessionId, isMounted]);

  useEffect(() => {
    if (!isMounted) return;
    let current: AgentActivity | null = null;
    let idleReadings = 0;
    // detectActivity is a pure function of (title, screen): while the terminal receives no
    // output and keeps its title, re-reading every row of the screen gives the same answer.
    // Idle and hidden panes therefore cost almost nothing per tick.
    let watched: Terminal | null = null;
    let writeSub: { dispose(): void } | null = null;
    let dirty = true;
    let lastTitle = "";
    let lastReading: AgentActivity | null = null;

    const timer = window.setInterval(() => {
      const term = termRef.current;
      if (!term) return;
      if (term !== watched) {
        writeSub?.dispose();
        watched = term;
        writeSub = term.onWriteParsed(() => {
          dirty = true;
        });
        dirty = true;
      }
      if (dirty || titleRef.current !== lastTitle) {
        dirty = false;
        lastTitle = titleRef.current;
        lastReading = detectActivity(kind, lastTitle, readScreen(term));
      }
      const next = lastReading;
      if (!next) return;
      idleReadings = next === "idle" ? idleReadings + 1 : 0;
      // A single idle reading between two spinner redraws must not flicker the icon.
      if (current === "working" && next === "idle" && idleReadings < 2) return;
      if (next !== current) {
        current = next;
        report(sessionId, next);
      }
    }, POLL_MS);

    return () => {
      window.clearInterval(timer);
      writeSub?.dispose();
      report(sessionId, null);
    };
  }, [sessionId, kind, isMounted, termRef, report]);

  return titleRef;
}
