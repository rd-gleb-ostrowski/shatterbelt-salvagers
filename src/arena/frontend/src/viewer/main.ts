/**
 * Viewer entry point — wires the ObserverClient and DriftRenderer together.
 *
 * On load:
 *   1. Initialises the PixiJS renderer in #viewer-canvas-container.
 *   2. Opens a WebSocket connection to the Arena's /observe stream.
 *   3. Calls renderer.renderFrame(frame) on each parsed god-view tick.
 *
 * Seams for future issues:
 *   02  — pass a camera controller to the renderer
 *   03  — ship team colours / labels / bars come from the renderer
 *   04  — Sigil effects / explosions come from the renderer
 *   05  — HUD overlay wired here (scoreboard, tick timer)
 *   06  — sound module wired here (receives same frame)
 *   07  — replay loader feeds the same renderFrame callback, bypassing wsClient
 */

import { DriftRenderer } from "./renderer.ts";
import { ObserverClient, defaultObserverUrl } from "./wsClient.ts";

async function init(): Promise<void> {
  const statusBar = document.getElementById("status-bar");
  const container = document.getElementById("viewer-canvas-container");

  if (!container) {
    console.error("[Viewer] #viewer-canvas-container not found");
    return;
  }

  const setStatus = (msg: string): void => {
    if (statusBar) statusBar.textContent = `⬡ ${msg}`;
  };

  setStatus("initialising renderer…");

  let renderer: DriftRenderer;
  try {
    renderer = await DriftRenderer.create(container);
  } catch (err) {
    setStatus(`renderer init failed: ${String(err)}`);
    console.error("[Viewer] renderer init failed", err);
    return;
  }

  const wsUrl = defaultObserverUrl();
  setStatus(`connecting to ${wsUrl}…`);

  const client = new ObserverClient({
    url: wsUrl,
    onFrame(frame) {
      renderer.renderFrame(frame);
    },
    onStatus: setStatus,
  });

  client.connect();
}

init().catch(console.error);
