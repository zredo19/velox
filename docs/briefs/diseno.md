# Encargo: diseño de Velox

Proyecto: `D:\Workspace\velox` (Tauri v2 + React + xterm.js). El líder de integración es la sesión
"Velox 2" (perfil trabajo). Hay otra sesión optimizando rendimiento en paralelo.

## Lo que pide el usuario

1. **Animaciones a 60 fps sin empeorar el rendimiento.** Que la de agy (`src/themes/agy/agyScene.ts`)
   sea más fluida, y la de Claude (`src/themes/claude/claudeScene.ts`) también si se puede. Revisa
   `src/themes/shared/scene.ts` (loop, dither, glyphs). Mide antes y después; no subas el uso de CPU/GPU
   (pausar cuando la ventana está oculta o sin foco, offscreen canvas, cachear glifos, etc.).
2. **Estrellas en el fondo:** chiquititas, blancas, sutiles (algunas con parpadeo suave).
3. **Íconos de Claude y agy un poco más brillantes.**
4. **Sidebar** (`src/components/Sidebar.tsx`): ahora se ve muy simple. Que también sea parte del
   fondo (que se vean las nubes moviéndose y las estrellas a través de ella), pero que se distinga del
   área de la terminal: una línea divisoria clara, sombra, vidrio esmerilado o lo que veas mejor.
5. **Abajo en la sidebar:** los íconos de Claude, agy, OpenCode y Codex, "felices" (con
   personalidad, alguna micro-animación liviana).
6. **Logo nuevo** (el actual es malo). Libertad artística: algo que transmita "una terminal con
   varios agentes". Va en la title bar (`src/components/TitleBar.tsx`) y, si puedes, en los íconos de
   la app (`src-tauri/icons`).
7. **Fondo gris de los mensajes enviados:** cuando el usuario manda un mensaje en Claude Code, ese
   bloque aparece con fondo gris opaco que tapa la animación. Que no salga, o que salga con poca
   opacidad para que se vea el fondo (el usuario prefiere esto último). Mira
   `src/lib/xtermTransparencyFix.ts` (el addon WebGL pinta opacos los fondos de celda no
   predeterminados).

No se puede perder ninguna funcionalidad ni animación existente.

## Reglas de coordinación (importante)

- **Tus archivos:** `src/themes/**`, `src/components/Sidebar.tsx` (solo lo visual),
  `src/components/TitleBar.tsx`, `src/components/icons/**`, `src/lib/xtermTransparencyFix.ts`, CSS e
  `index.html`, `src-tauri/icons`.
- **No toques** `src-tauri/src/**`, `src/hooks/useSessions.ts`, `src/App.tsx` ni la lógica de
  `src/components/TerminalPane.tsx`. Si necesitas cambiar algo ahí, pídelo antes.
- No hagas commits, no cierres `velox.exe` y no compiles en `src-tauri/target`. Para verificar, usa
  `pnpm exec tsc --noEmit`. Si necesitas un binario, usa `cargo build --release --features
  custom-protocol --target-dir target-design`.
- Al terminar, entrega un resumen: archivos cambiados, qué hiciste y cómo lo mediste (fps y CPU antes
  y después).
