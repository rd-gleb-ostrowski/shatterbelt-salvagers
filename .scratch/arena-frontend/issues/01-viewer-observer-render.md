# Observer-stream wiring & basic Drift render

Status: ready-for-agent
Type: AFK
User stories: 1, 5

## Parent

`.scratch/arena-frontend/PRD.md`

## What to build

The foundation of the **Viewer**: connect to the Arena Server's observer "god-mode" WS stream and
draw the Drift with PixiJS (WebGL) so the current live match is watchable on the projector.

- Subscribe to the server's observer god-mode stream and render each tick.
- Draw all world entities clearly: salvage **ships**, **relics** (glowing), **Anchors** (team home
  beacons), and **asteroids**.
- Render at a fixed default view (full Drift visible is acceptable for this slice; the flexible
  camera is the next slice).
- Aesthetic is the sci-fi/fantasy blend (aether glow, runic accents); Canvas2D is an acceptable
  fallback to PixiJS if needed.

Ship detailing (team colours, bars, shimmer), Sigil/explosion effects, HUD, and sound are separate
slices that build on this render loop.

## Acceptance criteria

- [ ] The Viewer connects to the server's observer god-mode WS stream and updates each tick.
- [ ] Ships, relics, Anchors, and asteroids are drawn in their correct world positions.
- [ ] Relics glow and Anchors read as team home beacons.
- [ ] The renderer keeps up smoothly with a live match on a projector-scale canvas.
- [ ] World↔screen coordinate mapping is a framework-free function covered by a unit test.

## Blocked by

None - can start immediately.
