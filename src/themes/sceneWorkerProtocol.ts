import type { Rect, SceneMode } from "./types";

export type SceneWorkerRequest =
  | { type: "init"; kind: string; canvases: OffscreenCanvas[] }
  | { type: "resize"; width: number; height: number; dpr: number; contentLeft: number }
  | { type: "mode"; mode: SceneMode }
  | { type: "dispose" };

export type SceneWorkerReply = { type: "layout"; boxes: Rect[]; backdrop: string };
