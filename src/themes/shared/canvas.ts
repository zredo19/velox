/**
 * Canvas helpers that work both on the main thread and inside the scene worker, where
 * there is no `document`: offscreen sprites are OffscreenCanvas whenever it exists.
 */

export type Ctx2D = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;
export type AnyCanvas = HTMLCanvasElement | OffscreenCanvas;

export function createCanvas(width: number, height: number): AnyCanvas {
  const w = Math.max(1, Math.ceil(width));
  const h = Math.max(1, Math.ceil(height));
  if (typeof OffscreenCanvas !== "undefined") return new OffscreenCanvas(w, h);
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  return canvas;
}

export function context2d(canvas: AnyCanvas): Ctx2D | null {
  return canvas.getContext("2d") as Ctx2D | null;
}
