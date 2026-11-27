import type { ITheme } from "@xterm/xterm";
import type { Ctx2D } from "./shared/canvas";

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** One canvas of a scene. Only what moves gets redrawn, each at its own pace. */
export interface SceneLayer {
  /**
   * Redraws per second while the window is focused. `0` means static: drawn only after a
   * resize. The loop is capped at about 60 fps.
   */
  fps: number;
  /** Area this layer paints, in CSS px of the whole background. Read after `resize`. */
  box(): Rect;
  /**
   * Draws one frame in whole-background CSS px coordinates. The caller has already set the
   * transform and cleared the layer's box. `live` is false while the window is unfocused
   * and the loop only steps a few times a second: layers that interpolate between cached
   * keyframes should then just draw the current moment.
   */
  draw(ctx: Ctx2D, timeSec: number, live: boolean): void;
}

/**
 * How much effort the scene may spend. `live`: focused window, ~60 fps. `slow`: unfocused,
 * a few steps a second. `paused`: hidden or minimised, nothing runs.
 */
export type SceneMode = "live" | "slow" | "paused";

/** An animated background scene drawn behind the terminal. */
export interface Scene {
  /**
   * Called on mount and every time the area or DPR changes. Sizes are CSS px. The scene
   * also shows through the sidebar: `contentLeft` is where the terminal area begins, so
   * the emblem and the mascot stay placed relative to it.
   */
  resize(width: number, height: number, dpr: number, contentLeft: number): void;
  /** Static CSS background the browser paints once. Read after `resize`. */
  backdrop(): string;
  /** Back to front. */
  layers: SceneLayer[];
}

export type CustomThemeId =
  | "tokyo-night"
  | "catppuccin"
  | "synthwave"
  | "nord"
  | "dracula"
  | "gruvbox"
  | "matrix";

export type AgentThemeId = "default" | "claude" | "agy" | "codex" | "opencode" | CustomThemeId;

export type ThemePreference = "auto" | AgentThemeId;

export interface ThemeMeta {
  id: ThemePreference;
  label: string;
  description: string;
  category: "auto" | "custom" | "agent";
  /** [canvas, surface, accent, accentSoft] preview dots */
  swatches: readonly [string, string, string, string];
}

export interface AgentTheme {
  /** Used as the `data-agent-theme` attribute on the app root. */
  id: AgentThemeId;
  /** Opaque CSS colour painted behind the canvas. */
  background: string;
  /** xterm palette. When `createScene` is set, `xterm.background` must be fully transparent. */
  xterm: ITheme;
  /** `null` means a plain static background. */
  createScene: (() => Scene) | null;
}
