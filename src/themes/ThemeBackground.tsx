import { memo, useEffect, useRef, useState } from "react";
import type { AgentTheme, Rect, SceneMode } from "./types";
import { runScene, type SceneLoop } from "./shared/sceneLoop";
import type { SceneWorkerReply, SceneWorkerRequest } from "./sceneWorkerProtocol";

const MAX_DPR = 1.5;

/* Attributes that let the scene escape <main> and also run behind the sidebar (see index.css). */
const ROW_ATTR = "data-velox-scene";
const MAIN_ATTR = "data-velox-scene-main";

const CAN_USE_WORKER =
  typeof Worker !== "undefined" &&
  typeof OffscreenCanvas !== "undefined" &&
  typeof HTMLCanvasElement !== "undefined" &&
  "transferControlToOffscreen" in HTMLCanvasElement.prototype;

/** Focused: ~60 fps. Unfocused: a few steps a second. Hidden / minimised: nothing. */
function currentMode(): SceneMode {
  if (document.hidden) return "paused";
  return document.hasFocus() ? "live" : "slow";
}

/** What the effect talks to: the scene worker, or the same loop on the main thread. */
interface Driver {
  resize(width: number, height: number, dpr: number, contentLeft: number): void;
  setMode(mode: SceneMode): void;
  dispose(): void;
}

function SceneLayers({ theme }: { theme: AgentTheme }) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    setReady(false);
    const host = hostRef.current;
    const createScene = theme.createScene;
    if (!host || !createScene) return;

    // The wallpaper covers the whole row (sidebar + terminal area): <main> stops being the
    // containing block, and the translucent sidebar shows the sky through it.
    const main = host.closest("main");
    const row = main?.parentElement ?? null;
    main?.setAttribute(MAIN_ATTR, "");
    row?.setAttribute(ROW_ATTR, "");

    // Deliberately ignores prefers-reduced-motion: Windows reports it whenever "animation
    // effects" are off for performance. The "Animated backgrounds" setting is the switch.
    const scene = createScene();
    let canvases: HTMLCanvasElement[] = [];
    let lastResize: [number, number, number, number] | null = null;
    let mode = currentMode();
    let revealed = false;

    const makeCanvases = () => {
      for (const canvas of canvases) canvas.remove();
      canvases = scene.layers.map(() => {
        const canvas = document.createElement("canvas");
        canvas.style.position = "absolute";
        host.appendChild(canvas);
        return canvas;
      });
    };

    const applyLayout = (boxes: Rect[], backdrop: string) => {
      host.style.background = `${backdrop}, ${theme.background}`;
      boxes.forEach((box, i) => {
        const style = canvases[i]?.style;
        if (!style) return;
        style.left = `${box.x}px`;
        style.top = `${box.y}px`;
        style.width = `${box.width}px`;
        style.height = `${box.height}px`;
      });
      if (!revealed) {
        revealed = true;
        setReady(true);
      }
    };

    const startOnMainThread = (): Driver => {
      makeCanvases();
      const loop: SceneLoop = runScene(scene, canvases, applyLayout);
      return loop;
    };

    const startWorker = (): Driver => {
      makeCanvases();
      const worker = new Worker(new URL("./sceneWorker.ts", import.meta.url), { type: "module" });
      const send = (message: SceneWorkerRequest, transfer: Transferable[] = []) =>
        worker.postMessage(message, transfer);
      worker.onmessage = ({ data }: MessageEvent<SceneWorkerReply>) => {
        if (data.type === "layout") applyLayout(data.boxes, data.backdrop);
      };
      // If the worker cannot start, the transferred canvases are lost: start over here.
      worker.onerror = () => {
        worker.terminate();
        driver = startOnMainThread();
        if (lastResize) driver.resize(...lastResize);
        driver.setMode(mode);
      };
      const offscreen = canvases.map((canvas) => canvas.transferControlToOffscreen());
      send({ type: "init", kind: theme.id, canvases: offscreen }, offscreen);
      return {
        resize: (width, height, dpr, contentLeft) =>
          send({ type: "resize", width, height, dpr, contentLeft }),
        setMode: (next) => send({ type: "mode", mode: next }),
        dispose: () => {
          send({ type: "dispose" });
          worker.terminate();
        },
      };
    };

    let driver: Driver = CAN_USE_WORKER ? startWorker() : startOnMainThread();

    let lastSize = "";
    let resizeTimer: number | undefined;
    const applyResize = () => {
      const { clientWidth: width, clientHeight: height } = host;
      if (width < 1 || height < 1) return;
      const dpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);
      const contentLeft = main
        ? Math.max(0, Math.round(main.getBoundingClientRect().left - host.getBoundingClientRect().left))
        : 0;
      // Both observed elements report the same change; rebuild once.
      const size = `${width}x${height}@${dpr}+${contentLeft}`;
      if (size === lastSize) return;
      lastSize = size;
      lastResize = [width, height, dpr, contentLeft];
      driver.resize(width, height, dpr, contentLeft);
    };

    const scheduleResize = () => {
      // First layout runs synchronously; subsequent resizes (e.g. sidebar transition or
      // window drag) are debounced so clouds/halo sprites are not rebuilt mid-animation.
      if (!lastSize) {
        applyResize();
        return;
      }
      window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(applyResize, 80);
    };

    const observer = new ResizeObserver(scheduleResize);
    observer.observe(host);
    // Toggling the sidebar resizes <main> but not the row.
    if (main) observer.observe(main);
    applyResize();
    driver.setMode(mode);

    const updateMode = () => {
      mode = currentMode();
      driver.setMode(mode);
    };
    window.addEventListener("focus", updateMode);
    window.addEventListener("blur", updateMode);
    document.addEventListener("visibilitychange", updateMode);

    return () => {
      window.clearTimeout(resizeTimer);
      window.removeEventListener("focus", updateMode);
      window.removeEventListener("blur", updateMode);
      document.removeEventListener("visibilitychange", updateMode);
      observer.disconnect();
      driver.dispose();
      for (const canvas of canvases) canvas.remove();
      main?.removeAttribute(MAIN_ATTR);
      row?.removeAttribute(ROW_ATTR);
    };
  }, [theme]);

  return (
    <div
      ref={hostRef}
      className={`absolute inset-0 overflow-hidden transition-opacity duration-300 ease-out ${
        ready ? "opacity-100" : "opacity-0"
      }`}
    />
  );
}

export const ThemeBackground = memo(function ThemeBackground({ theme }: { theme: AgentTheme }) {
  if (!theme.createScene) return null;

  return (
    <div aria-hidden="true" className="velox-scene absolute inset-0 pointer-events-none">
      <SceneLayers theme={theme} />
    </div>
  );
});
