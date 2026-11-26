# Velox — Claude animated theme (design spec)

Goal: sessions whose `agentKind === "claude"` show a warm dark theme with an animated
background behind the terminal text. Every other agent keeps today's look.
Antigravity (`agy`) has the same wallpaper in its own colours: its gradient arch instead of the
burst, hovering with thrust trails, and a floating bot instead of the crab.

## 1. Reference analysis

### 1.1 The owner's portfolio (animation STYLE to copy)
Read these two files, they are the style source of truth:
- `D:\Workspace\portafolio\src\components\canvas\DitherSky.tsx`
- `D:\Workspace\portafolio\src\lib\dither.ts`

Technique: plain Canvas 2D, no libraries. A coarse cell grid; every "on" cell is a small
square dot; soft fields (sun halo, clouds) are thresholded with a 4x4 Bayer matrix.
Sun/moon halo breathes (`0.92 + 0.08*sin(t*1.4)`), halo falls off exponentially,
clouds are unions of puffs with flat bottoms drifting and wrapping horizontally,
stars twinkle with per-star phase/speed, deterministic PRNG `mulberry32`,
`prefers-reduced-motion` renders one static frame, capped frame rate.

### 1.2 The TikTok reference (COMPOSITION to copy, "Claude Code" wallpaper)
- Very dark warm brown canvas with a slightly warmer glow toward the corners.
- A big Claude starburst (about 12 irregular rays) in the upper right, built from a grid of
  small square blocks; each block carries a tiny darker ASCII glyph (`t % S X ; : .`).
  Centre cells are pale cream-pink, mid cells coral, tips darker brick.
- Faint scattered ASCII glyphs and single pixels (`{ < ; + % . :`) across the canvas.
- A small coral pixel mascot (wide blocky crab-like creature, two eyes, little legs)
  that idles and bobs near the bottom right.

## 2. Visual spec (put these numbers in named constants)

- Cell pitch 10 CSS px, block 9 px.
- Palette:
  - base `#17120F`, corner glow `rgba(217,119,87,0.07)`
  - ray centre `#EBC7B4`, coral `#D97757`, brick `#8E4A38`
  - glyph ink `rgba(23,18,15,0.55)`
  - halo dots `rgba(217,119,87,0.35)`, cloud dots `rgba(184,168,158,0.10)` / `0.17`
  - mascot `#D97757`, eyes = base colour
- Starburst: centre (78% width, 30% height), radius `clamp(90, width*0.11, 190)`.
  Motion: very slow rotation (about one turn per 3 minutes), each ray's length breathes
  +-10% with its own phase, a few glyphs swap every ~600 ms. Global alpha about 0.75 so
  terminal text stays readable on top.
- Halo: Bayer-dithered dots around the burst, breathing like the portfolio's sun.
- Clouds: 3 to 4 faint dithered clouds drifting slowly in the upper half, like the portfolio.
- Ambient: about 60 faint glyphs/pixels, twinkling.
- Mascot: pixel sprite about 56x40 CSS px, 96 px above the bottom, 48 px from the right edge.
  Bobs 2 px, blinks every ~4 s, shifts legs, and slowly strolls +-40 px.
- Narrow canvases (< 640 px wide): radius 70, hide clouds.

## 3. Performance and accessibility (hard requirements)

- Velox is a low-RAM app. Only the active session has a background: one static CSS glow
  plus one canvas per layer (sky 4 fps, star 15 fps, mascot 7.5 fps), each only as big as
  what moves in it. A timer drives them, never requestAnimationFrame.
- Max 15 fps (5 when the window is not focused). No frames while `document.hidden`. Cancel the rAF loop on unmount.
- DPR capped at 1.5.
- Never call `fillText` per cell per frame: pre-render glyph blocks once into an offscreen
  sprite atlas and use `drawImage`. Allocate nothing per cell inside `draw`.
- `prefers-reduced-motion` is deliberately ignored (Windows reports it when "animation effects"
  are off for performance); the "Animated backgrounds" setting is the switch.
- The canvas is `aria-hidden` and `pointer-events: none`.
- No new npm dependencies.

## 4. Where it lives

- `src/themes/types.ts` — `Scene` and `AgentTheme`.
- `src/themes/registry.ts` — `getAgentTheme(kind, animated)`; add new agent themes here.
- `src/themes/ThemeBackground.tsx` — the single canvas and its frame loop (15 fps focused, 5 fps unfocused).
- `src/themes/shared/` — the scene engine every agent uses (sky, halo, layers, mascot walk).
- `src/themes/claude/`, `src/themes/agy/` — each agent's emblem, mascot and colours.
- `src/lib/agentActivity.ts` — sidebar icon states (rules ported from herdr).
- Chrome colours: `--velox-*` variables in `src/index.css`, overridden by `[data-agent-theme="claude"]`.
- Setting: `animatedBackgrounds` (`animated_backgrounds` in `config.toml`).
