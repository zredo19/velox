/**
 * Draws the animated background off the main thread. The canvases arrive transferred
 * (OffscreenCanvas), so every frame is committed from here straight to the compositor:
 * the main thread runs no animation frames, layout or paint for the wallpaper.
 */
import { createAgyScene } from "./agy/agyScene";
import { createClaudeScene } from "./claude/claudeScene";
import { createCodexScene } from "./codex/codexScene";
import { createOpencodeScene } from "./opencode/opencodeScene";
import { runScene, type SceneLoop } from "./shared/sceneLoop";
import type { Scene } from "./types";
import type { SceneWorkerReply, SceneWorkerRequest } from "./sceneWorkerProtocol";

const SCENES: Record<string, () => Scene> = {
  agy: createAgyScene,
  claude: createClaudeScene,
  codex: createCodexScene,
  opencode: createOpencodeScene,
};

const port = self as unknown as {
  postMessage(message: SceneWorkerReply): void;
  onmessage: ((event: MessageEvent<SceneWorkerRequest>) => void) | null;
};

let loop: SceneLoop | null = null;

port.onmessage = ({ data }) => {
  switch (data.type) {
    case "init": {
      const create = SCENES[data.kind];
      if (!create) return;
      loop = runScene(create(), data.canvases, (boxes, backdrop) =>
        port.postMessage({ type: "layout", boxes, backdrop }),
      );
      break;
    }
    case "resize":
      loop?.resize(data.width, data.height, data.dpr, data.contentLeft);
      break;
    case "mode":
      loop?.setMode(data.mode);
      break;
    case "dispose":
      loop?.dispose();
      loop = null;
      break;
  }
};
