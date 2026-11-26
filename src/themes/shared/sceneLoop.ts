/**
 * Drives a scene's layers. Runs inside the scene worker (the normal path: frames go
 * straight from the worker to the compositor, so the main thread, where xterm lives, does
 * no per-frame work at all) or, as a fallback, on the main thread.
 *
 * `live`: the loop draws on requestAnimationFrame (vsync aligned, so motion is even) at
 * about 60 fps whatever the refresh rate; on a 144 Hz screen that is two or three vsyncs
 * per frame. Between frames it sleeps on a timer instead of waking for every vsync.
 * `slow`: a few steps a second. `paused`: nothing runs.
 */
import type { Rect, Scene, SceneMode } from "../types";
import { context2d, type AnyCanvas } from "./canvas";

const FRAME_MS = 1000 / 60;
/* Within this much of a frame counts as due, so timer and vsync jitter never skip one. */
const FRAME_SLACK_MS = 4;
/* How long before the next frame is due the timer hands over to requestAnimationFrame. */
const WAKE_EARLY_MS = 6;
/** Keeps moving, but cheaper, while the user is working in another window. */
const SLOW_TICK_MS = 1000 / 5;

export interface SceneLoop {
  resize(width: number, height: number, dpr: number, contentLeft: number): void;
  setMode(mode: SceneMode): void;
  dispose(): void;
}

/**
 * `onLayout` receives each layer's box (CSS px) and the CSS backdrop after every resize, so
 * the owner of the canvas elements can position them.
 */
export function runScene(
  scene: Scene,
  canvases: AnyCanvas[],
  onLayout: (boxes: Rect[], backdrop: string) => void,
): SceneLoop {
  const layers = scene.layers.map((layer, i) => ({
    layer,
    canvas: canvases[i],
    ctx: canvases[i] ? context2d(canvases[i]) : null,
    due: 0,
  }));
  let dpr = 1;
  let ready = false;
  let mode: SceneMode = "paused";
  let raf = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let lastFrame = -Infinity;

  const render = (now: number, force: boolean, live: boolean) => {
    if (!ready) return;
    for (const item of layers) {
      if (!item.ctx) continue;
      if (!force && (item.layer.fps <= 0 || now < item.due - FRAME_SLACK_MS)) continue;
      // Scheduled from the previous due time, so a 15 fps layer stays at 15 on a 60 fps
      // loop instead of slipping a frame every time.
      const step = 1000 / Math.max(1, item.layer.fps);
      item.due = now - item.due < step ? item.due + step : now + step;
      const box = item.layer.box();
      item.ctx.setTransform(dpr, 0, 0, dpr, -box.x * dpr, -box.y * dpr);
      item.ctx.clearRect(box.x, box.y, box.width, box.height);
      item.layer.draw(item.ctx, now / 1000, live);
    }
  };

  const stop = () => {
    if (raf) cancelAnimationFrame(raf);
    clearTimeout(timer);
    raf = 0;
    timer = undefined;
  };

  const onFrame = (now: number) => {
    raf = 0;
    if (mode !== "live") return;
    if (now - lastFrame < FRAME_MS - FRAME_SLACK_MS) {
      raf = requestAnimationFrame(onFrame);
      return;
    }
    lastFrame = now;
    render(now, false, true);
    const wait = FRAME_MS - (performance.now() - now) - WAKE_EARLY_MS;
    timer = setTimeout(() => {
      timer = undefined;
      if (mode === "live") raf = requestAnimationFrame(onFrame);
    }, Math.max(0, wait));
  };

  const onSlowTick = () => {
    timer = undefined;
    if (mode !== "slow") return;
    render(performance.now(), false, false);
    timer = setTimeout(onSlowTick, SLOW_TICK_MS);
  };

  const schedule = () => {
    stop();
    if (mode === "live") raf = requestAnimationFrame(onFrame);
    else if (mode === "slow") timer = setTimeout(onSlowTick, SLOW_TICK_MS);
  };

  return {
    resize(width, height, nextDpr, contentLeft) {
      dpr = nextDpr;
      scene.resize(width, height, dpr, contentLeft);
      const boxes = layers.map(({ layer, canvas }) => {
        const box = layer.box();
        if (canvas) {
          canvas.width = Math.max(1, Math.round(box.width * dpr));
          canvas.height = Math.max(1, Math.round(box.height * dpr));
        }
        return box;
      });
      onLayout(boxes, scene.backdrop());
      ready = true;
      render(performance.now(), true, false);
    },

    setMode(next) {
      if (next === mode) return;
      mode = next;
      schedule();
    },

    dispose() {
      mode = "paused";
      stop();
      ready = false;
    },
  };
}
