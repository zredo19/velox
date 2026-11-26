# Resultado: encargo de diseño de Velox

Hecho por la pestaña "Workspace". No hice commits, no cerré `velox.exe` y no compilé Rust. La
verificación fue `pnpm exec tsc --noEmit`, que pasa sin errores. No toqué `src-tauri/src/**`,
`useSessions.ts`, `App.tsx` ni la lógica de `TerminalPane.tsx`.

## Archivos

**Modificados**
- `src/themes/types.ts`: `SceneLayer.draw(ctx, t, live)`, `fps: 0` = capa estática,
  `Scene.resize(w, h, dpr, contentLeft)` y el tipo `SceneMode`.
- `src/themes/ThemeBackground.tsx`: crea los canvas, los transfiere al Worker (con respaldo en el
  hilo principal), maneja los modos foco / sin foco / oculta y deja que la escena salga de `<main>`.
- `src/themes/shared/scene.ts`: estrellas, nubes suaves, keyframes del emblema con fundido
  cruzado, sprite de la mascota cacheado y atlas de glifos.
- `src/themes/shared/glyphs.ts`: usa OffscreenCanvas (funciona dentro del worker).
- `src/themes/agy/agyScene.ts`: la flotación pasa a hacerse por frame, la mascota se cachea por
  pose, y el emblema y el halo quedan un poco más brillantes.
- `src/themes/claude/claudeScene.ts`: las celdas polares se precalculan (ya no hay hypot/atan2/Bayer
  por frame), el burst queda más brillante (alpha de 0.75 a 0.86) y el cangrejo se cachea por pose.
- `src/components/Sidebar.tsx`: solo cambios visuales: clase `velox-sidebar`, clases
  `velox-row(-active)` y `<AgentCrew />` al pie.
- `src/components/icons/AgentIcons.tsx`: íconos de Claude y agy más brillantes, con un halo suave,
  y el logo `VeloxLogo` nuevo.
- `src/lib/xtermTransparencyFix.ts`: los fondos de celda explícitos son translúcidos cuando el tema
  es transparente.
- `src/index.css`: reglas de la escena detrás de la sidebar, vidrio de la sidebar y animaciones de
  la tripulación.
- `src-tauri/icons/32x32.png`, `128x128.png` e `icon.ico` (16 a 256 px): el logo nuevo.

**Nuevos**
- `src/themes/shared/canvas.ts`: `createCanvas`/`context2d`, que funcionan igual en el hilo
  principal y en el worker.
- `src/themes/shared/sceneLoop.ts`: el loop compartido entre el worker y el respaldo.
- `src/themes/sceneWorker.ts` y `src/themes/sceneWorkerProtocol.ts`: worker con OffscreenCanvas.
- `src/components/icons/AgentCrew.tsx`: los 4 personajes "felices".

`TitleBar.tsx` ya aparece modificado en git, pero ese cambio no es mío. El logo nuevo le llega a
través de `VeloxLogo`, así que no lo toqué.

## Qué hice, por punto

1. **Animaciones a 60 fps.** El dibujo pasó a un **Web Worker con OffscreenCanvas**: los frames
   van del worker directo al compositor y el hilo de xterm no hace trabajo por frame.
   - El loop sigue `requestAnimationFrame` con un tope de ~60 fps. En la pantalla de 144 Hz del
     usuario dibuja cada 2 vsync, es decir 72 fps parejos. Entre frames duerme en un timer, así que
     no despierta en cada vsync.
   - El emblema (cientos de glifos) se sigue renderizando a 15 keyframes/s, como antes. Cada frame
     mezcla los dos keyframes más cercanos con `lighter`, que es una interpolación lineal exacta.
     Así la respiración del burst de Claude, el barrido de luz y el pulso del halo se ven fluidos y
     cada frame cuesta dos `drawImage`.
   - La flotación de agy y el paseo y rebote de las mascotas se calculan por frame y se ajustan al
     píxel del dispositivo, para que el pixel art no se vea borroso.
   - Las nubes ya no saltan de 6 px en 6 px: avanzan de a un píxel del dispositivo, a 15 fps.
   - Ventana sin foco: 5 pasos por segundo (igual que antes). Ventana oculta o minimizada: no corre
     nada (antes se despertaba 1 vez por segundo).
   - Probé y **descarté** CSS/Web Animations en el compositor. Corren a la tasa de refresco del
     monitor (144 fps) y el CPU del proceso GPU subía a 57–70%.
2. **Estrellas.** Son de 1–2 píxeles del dispositivo, blancas y con alpha entre 0.24 y 0.6. Hay
   más arriba que abajo. Unas 480 pueden caer sobre una ventana de 1600×900 (hasta 520). Las
   estáticas se pintan una vez por resize. El 16% parpadea suave y algunas tienen un destello en
   cruz cuando llegan al máximo. Usan una semilla fija, así que no cambian de lugar al redimensionar.
3. **Íconos más brillantes.**
   - Claude: `#D97757` → `#F08F69`, con un halo suave. Agy: brightness 1.18, saturate 1.12 y halo.
   - Estados apagados: Claude más claro y agy con opacidad 0.62 (antes 0.5).
   - Los emblemas del fondo también suben un poco: Claude 0.75 → 0.86, agy unos +10%, y el halo
     algo más fuerte.
4. **Sidebar.**
   - La escena ahora cubre toda la fila (sidebar + terminal), así que las nubes y las estrellas se
     ven a través de la sidebar. El emblema y la mascota siguen ubicados respecto del área de la
     terminal (`contentLeft`).
   - La sidebar es vidrio tintado: el fondo de la superficie va del 44% de opacidad arriba al 72%
     abajo. Tiene una línea divisoria clara con un brillo del color de acento y una sombra hacia la
     terminal. Las filas activas y con hover también son translúcidas.
   - **No usé blur:** borraría las estrellas, que miden un píxel, y obligaría a re-filtrar el fondo
     en cada frame.
   - Si no hay escena (tema default, sesión dormida o animación desactivada), la sidebar se ve
     opaca como antes.
5. **Tripulación al pie de la sidebar.** Son cuatro personajes de pixel art en SVG: el cangrejo de
   Claude, el robot de agy, el diamante de OpenCode y el hexabot de Codex. Todos sonríen, están
   parados sobre una línea de "suelo" y tienen un halo suave del color de su marca.
   - Cada 2.6–5.2 s uno salta o parpadea, y a veces saltan todos en ola. Al pasar el mouse, ese
     personaje salta.
   - Es un timer que agrega una animación CSS corta, así que entre animaciones no corre nada. Se
     detiene si la ventana está oculta o sin foco. Son decorativos, sin clic.
6. **Logo.** Una ventana de terminal con un prompt `>` del que salen tres líneas de salida, una por
   agente (naranja Claude, azul agy, verde Codex), con un cursor al final, y un borde en degradé de
   los tres colores. Está en la title bar, vía `VeloxLogo`, y en `src-tauri/icons`. Para que el
   ícono del .exe cambie hay que recompilar.
7. **Fondo gris de los mensajes.** El addon WebGL pintaba opaco cualquier fondo de celda explícito.
   Ahora, solo cuando el tema es transparente (hay escena):
   - Los grises neutros quedan al 30% de opacidad y los fondos de color (diffs, resaltados) al 55%.
   - Inverse video (cursor, ítems seleccionados) sigue opaco. El arreglo anterior del dim/italic se
     mantiene.
   - El addon mezcla los colores de una forma que distorsiona el alpha, así que el patch le pasa
     `sqrt(alpha)` y `rgb·sqrt(alpha)` para que la opacidad final sea exactamente la indicada.
   - Lo verifiqué con xterm + WebGL reales sobre un fondo de color, comparando antes y después.

No se perdió ninguna animación: siguen la respiración, el giro y el barrido del burst; el shimmer
de glifos; la flotación y las estelas de agy; las chispas; el halo; las nubes; los glifos que
"respiran"; y la mascota con sus poses, parpadeo, propulsores y luces. También siguen las
animaciones de los íconos de la sidebar cuando un agente trabaja.

## Cómo lo medí

Usé un banco en Chrome 1600×640 a 150% de escala, con la pantalla real de 144 Hz. Monta el
`ThemeBackground` real con la misma estructura de `App` (fila + aside + main). Para la versión
anterior usé una copia exacta de los archivos originales.
- Por CDP medí el tiempo de tareas del hilo principal (`TaskDuration`) y los frames: capas
  redibujadas por segundo en la versión base, y `FireAnimationFrame` del worker en la nueva.
- Para el CPU total sumé el tiempo de CPU de todos los procesos de esa instancia de Chrome
  (renderer, GPU y browser) en ventanas de 20 s.
- Hice 3 corridas intercaladas por variante, con el foco forzado.

| | antes | después |
|---|---|---|
| fps del emblema (agy / Claude) | ~13 (timer de 15 con jitter); mascota 6.7; nubes 4 | **72** (todas las capas que se mueven); nubes, glifos y parpadeo 15 |
| Hilo principal (tareas, ms/s) | agy 30–31, Claude 33 | **~2** (agy y Claude) |
| JS de la escena en el hilo principal | 22–26 ms/s | ~0 (corre en el worker) |
| CPU total de Chrome (% de 1 núcleo) | agy 19–22, Claude 18–21 | agy 38–47, Claude 30–34 |
| Piso sin escena | 6–11 | — |

**Lectura honesta:**
- El hilo principal, donde vive xterm, quedó casi libre: ~30 ms/s → ~2 ms/s.
- El CPU total sube **unos +10 a +25 puntos de un núcleo**. Es lo que cuesta presentar ~72 frames
  por segundo en vez de ~13: el costo por frame del proceso GPU (Intel UHD) es fijo, y el JS ya no
  es lo que pesa.
- En un equipo de 8 hilos eso equivale a ~1.5–3% del CPU total. Solo pasa con la ventana enfocada;
  sin foco baja a 5 fps y oculta no gasta nada.
- La medición de CPU total es ruidosa en esta máquina: la misma variante dio entre 29% y 52%.

Si el líder prefiere no subir el CPU, `FRAME_MS` en `src/themes/shared/sceneLoop.ts` es el ajuste.
- 1000/30 da ~36 fps en 144 Hz y quedaría cerca del costo anterior. El emblema sigue fluido gracias
  al fundido entre keyframes.
- Probé 48 fps, pero el ruido no dejó demostrar una ventaja clara.

## Notas para integrar

- La escena sale de `<main>` poniendo atributos desde `ThemeBackground`: `data-velox-scene` en la
  fila y `data-velox-scene-main` en `<main>`. Con eso, `index.css` deja `<main>` en
  `position: static` y fondo transparente.
- La sidebar pasa a `z-index: 20`: queda por encima de la escena y del `TerminalPane` (`z-10`),
  para que su menú contextual no quede debajo de la terminal.
- Esto depende de que `App.tsx` siga renderizando `<ThemeBackground>` dentro de `<main>`, y `<main>`
  dentro de la misma fila que `<Sidebar>`. Si otra sesión cambia esa estructura, hay que avisarme.
  Una alternativa más limpia, que requiere tocar `App.tsx`, sería renderizar `ThemeBackground`
  directamente en la fila.
- El worker se carga con `new Worker(new URL("./sceneWorker.ts", import.meta.url), { type: "module" })`.
  Vite lo empaqueta sin configuración. Si WebView2 no puede iniciarlo, la escena cae sola al hilo
  principal con el mismo loop.
- El ícono del .exe solo cambia al recompilar. No lo compilé.
