# Arena Frontend — Viewer & Admin

TypeScript + Vite multi-page app.

## Quick start

```bash
cd /workspace/src/arena/frontend
npm install
npm run dev
```

Open [http://localhost:5173](http://localhost:5173) for the Viewer,
[http://localhost:5173/admin.html](http://localhost:5173/admin.html) for the Admin stub.

## Pointing the Viewer at the Arena server

The Viewer connects to the Arena's observer WebSocket stream at `/observe`.  
By default it derives the WS URL from the page's host: `ws://<host>/observe`.

**Dev (server running elsewhere):** set `window.ARENA_WS_URL` before the page loads,
or open the browser console after load and run:

```js
window.ARENA_WS_URL = 'ws://localhost:3000/observe';
location.reload();
```

Or use Vite's proxy in `vite.config.ts`:

```ts
server: {
  proxy: {
    '/observe': { target: 'ws://localhost:3000', ws: true },
  },
},
```

**Production:** the Arena server serves the built static assets from `dist/`
(see `FRONTEND.md`) and the Viewer connects to the same host — no config needed.

## Scripts

| Script | What it does |
|---|---|
| `npm run dev` | Vite dev server (HMR) |
| `npm run build` | TypeScript typecheck + Vite production build |
| `npm test` | Vitest unit tests (pure-logic modules only) |

## Project layout

```
src/
  lib/
    worldTransform.ts        pure world↔screen transform (unit-tested)
    worldTransform.test.ts
    frameParser.ts           god-view frame JSON → typed model (unit-tested)
    frameParser.test.ts
  viewer/
    main.ts                  Viewer entry point
    wsClient.ts              live WebSocket client → /observe
    renderer.ts              PixiJS DriftRenderer
  admin/
    main.ts                  Admin stub (issue 08)
index.html                   Viewer page
admin.html                   Admin page
```

## Seams for future issues

| Issue | Seam |
|---|---|
| 02 (camera) | Replace `fitDriftTransform` in `renderer.ts` with `camera.getTransform()`; camera exposes the same `CameraTransform` shape and is driven by keyboard/mouse |
| 03 (ship cues) | Extend `drawShips` in `renderer.ts` — add team colours from scores keys, hull/shield bars, name labels, thrust flames; all driven by the existing `GodShipView` fields |
| 04 (Sigil/explosion effects) | Add `drawEffects` layer in `renderer.ts` fed by `frame.singularities`, `frame.mines`; explosion sprites triggered by events in `GodViewFrame` (events field not yet in god-view — note below) |
| 05 (HUD) | Mount a DOM overlay (or PixiJS Text) in `main.ts`; fed by `frame.scores`, `frame.tick`, `frame.maxTicks` |
| 06 (sound) | Add a `SoundManager` in `src/viewer/sound.ts`; wire it in `main.ts` alongside `renderFrame` — same `GodViewFrame` is passed to both |
| 07 (replay) | Replace `ObserverClient` in `main.ts` with a `ReplayPlayer` that implements the same `onFrame` callback, feeding recorded frames through `parseGodViewFrame` |

## Note on server/docs mismatch

The `GodViewFrameJson` in `observer.rs` does **not** include an `events` field
(unlike the per-bot `TickMsg` in `ws.rs` which carries `Vec<EventJson>`).
The sound module (issue 06) will need the server to add events to the god-view
frame, or derive audio cues from state deltas.  No server files were modified.
