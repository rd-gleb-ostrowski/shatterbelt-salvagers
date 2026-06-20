/**
 * frameParser — framework-free god-view frame JSON → typed model.
 *
 * Mirrors the server's camelCase wire shape exactly (GodViewFrameJson in
 * observer.rs / ws.rs).  Returns `null` on any validation failure so the
 * renderer can safely skip malformed frames.
 *
 * Seam for issues 06 (sound), 07 (replay): both consume `GodViewFrame`;
 * the live WS client and the replay loader both call `parseGodViewFrame`.
 */

// ── God-view event types (new authoritative event stream) ─────────────────────

/**
 * Discriminated union of all god-view frame events.
 * Each entry carries `ship` (the SUBJECT ship id) + `event` (camelCase tag)
 * + any payload fields defined in the server protocol.
 */
export type GodEvent =
  | { ship: string; event: "tookShield"; amount: number; by: string }
  | { ship: string; event: "tookHull"; amount: number; by: string }
  | { ship: string; event: "shieldDown" }
  | { ship: string; event: "lanceTookHull"; amount: number; by: string }
  | { ship: string; event: "collisionTookShield"; amount: number }
  | { ship: string; event: "collisionTookHull"; amount: number }
  | { ship: string; event: "relicDropped"; relicId: string; pos: Vec2 }
  | { ship: string; event: "relicTaken" }
  | { ship: string; event: "relicBanked"; value: number }
  | { ship: string; event: "cannonFired" }
  | { ship: string; event: "sigilGranted"; which: string }
  | { ship: string; event: "sigilDischarged"; which: string }
  | { ship: string; event: "afterburnerExpired" }
  | { ship: string; event: "bulwarkExpired" }
  | { ship: string; event: "singularityDeployed"; id: string; pos: Vec2 }
  | { ship: string; event: "mineDeployed"; id: string; pos: Vec2 }
  | { ship: string; event: "mineDetonated"; mineId: string; pos: Vec2 }
  | { ship: string; event: "killedShip"; victim: string }
  | { ship: string; event: "died"; by: string | null }
  | { ship: string; event: "respawned" };

// ── Domain value types (mirror the server's camelCase JSON) ──────────────────

export interface Vec2 {
  x: number;
  y: number;
}

export interface Resource {
  cur: number;
  max: number;
}

export interface ArenaDims {
  width: number;
  height: number;
}

export interface GodShipView {
  id: string;
  class: string;
  alive: boolean;
  invuln: boolean;
  pos: Vec2;
  vel: Vec2;
  heading: number;
  angVel: number;
  hull: Resource;
  shield: Resource;
  aether: Resource;
  sigil: string | null;
  cannonCooldown: number;
  relicsCarried: number;
  afterburnerTicksLeft: number;
}

export interface AnchorView {
  shipId: string;
  pos: Vec2;
}

export interface RelicView {
  id: string;
  pos: Vec2;
  vel: Vec2;
  value: number;
}

export interface AsteroidView {
  id: string;
  pos: Vec2;
  vel: Vec2;
  radius: number;
}

export interface ProjectileView {
  id: string;
  pos: Vec2;
  vel: Vec2;
  owner: string;
}

export interface SingularityView {
  id: string;
  pos: Vec2;
  radius: number;
  ticksLeft: number;
}

export interface MineView {
  id: string;
  pos: Vec2;
  own: boolean;
}

/**
 * Fully-parsed, typed god-view frame — the shape the renderer and all
 * downstream consumers (sound, replay) operate on.
 */
export interface GodViewFrame {
  type: "godView";
  tick: number;
  maxTicks: number;
  seed: number;
  arena: ArenaDims;
  ships: GodShipView[];
  anchors: AnchorView[];
  relics: RelicView[];
  asteroids: AsteroidView[];
  projectiles: ProjectileView[];
  singularities: SingularityView[];
  mines: MineView[];
  scores: Record<string, number>;
  /** Authoritative event log for this tick. Empty array if no events occurred. */
  events: GodEvent[];
}

// ── Parser helpers ────────────────────────────────────────────────────────────

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function parseVec2(v: unknown): Vec2 | null {
  if (!isObject(v)) return null;
  if (typeof v.x !== "number" || typeof v.y !== "number") return null;
  return { x: v.x, y: v.y };
}

function parseResource(v: unknown): Resource | null {
  if (!isObject(v)) return null;
  if (typeof v.cur !== "number" || typeof v.max !== "number") return null;
  return { cur: v.cur, max: v.max };
}

function parseArenaDims(v: unknown): ArenaDims | null {
  if (!isObject(v)) return null;
  if (typeof v.width !== "number" || typeof v.height !== "number") return null;
  return { width: v.width, height: v.height };
}

function parseShip(v: unknown): GodShipView | null {
  if (!isObject(v)) return null;
  const pos = parseVec2(v.pos);
  const vel = parseVec2(v.vel);
  const hull = parseResource(v.hull);
  const shield = parseResource(v.shield);
  const aether = parseResource(v.aether);
  if (!pos || !vel || !hull || !shield || !aether) return null;
  if (typeof v.id !== "string") return null;
  if (typeof v.class !== "string") return null;
  if (typeof v.alive !== "boolean") return null;
  if (typeof v.invuln !== "boolean") return null;
  if (typeof v.heading !== "number") return null;
  if (typeof v.angVel !== "number") return null;
  if (typeof v.cannonCooldown !== "number") return null;
  if (typeof v.relicsCarried !== "number") return null;
  if (typeof v.afterburnerTicksLeft !== "number") return null;
  const sigil =
    v.sigil === null || v.sigil === undefined
      ? null
      : typeof v.sigil === "string"
        ? v.sigil
        : null;
  return {
    id: v.id,
    class: v.class,
    alive: v.alive,
    invuln: v.invuln,
    pos,
    vel,
    heading: v.heading,
    angVel: v.angVel,
    hull,
    shield,
    aether,
    sigil,
    cannonCooldown: v.cannonCooldown,
    relicsCarried: v.relicsCarried,
    afterburnerTicksLeft: v.afterburnerTicksLeft,
  };
}

function parseAnchor(v: unknown): AnchorView | null {
  if (!isObject(v)) return null;
  if (typeof v.shipId !== "string") return null;
  const pos = parseVec2(v.pos);
  if (!pos) return null;
  return { shipId: v.shipId, pos };
}

function parseRelic(v: unknown): RelicView | null {
  if (!isObject(v)) return null;
  if (typeof v.id !== "string") return null;
  if (typeof v.value !== "number") return null;
  const pos = parseVec2(v.pos);
  const vel = parseVec2(v.vel);
  if (!pos || !vel) return null;
  return { id: v.id, pos, vel, value: v.value };
}

function parseAsteroid(v: unknown): AsteroidView | null {
  if (!isObject(v)) return null;
  if (typeof v.id !== "string") return null;
  if (typeof v.radius !== "number") return null;
  const pos = parseVec2(v.pos);
  const vel = parseVec2(v.vel);
  if (!pos || !vel) return null;
  return { id: v.id, pos, vel, radius: v.radius };
}

function parseProjectile(v: unknown): ProjectileView | null {
  if (!isObject(v)) return null;
  if (typeof v.id !== "string") return null;
  if (typeof v.owner !== "string") return null;
  const pos = parseVec2(v.pos);
  const vel = parseVec2(v.vel);
  if (!pos || !vel) return null;
  return { id: v.id, pos, vel, owner: v.owner };
}

function parseSingularity(v: unknown): SingularityView | null {
  if (!isObject(v)) return null;
  if (typeof v.id !== "string") return null;
  if (typeof v.radius !== "number") return null;
  if (typeof v.ticksLeft !== "number") return null;
  const pos = parseVec2(v.pos);
  if (!pos) return null;
  return { id: v.id, pos, radius: v.radius, ticksLeft: v.ticksLeft };
}

function parseMine(v: unknown): MineView | null {
  if (!isObject(v)) return null;
  if (typeof v.id !== "string") return null;
  if (typeof v.own !== "boolean") return null;
  const pos = parseVec2(v.pos);
  if (!pos) return null;
  return { id: v.id, pos, own: v.own };
}

function parseArray<T>(
  raw: unknown,
  parser: (item: unknown) => T | null
): T[] | null {
  if (!Array.isArray(raw)) return null;
  const result: T[] = [];
  for (const item of raw) {
    const parsed = parser(item);
    if (parsed === null) return null;
    result.push(parsed);
  }
  return result;
}

function parseScores(raw: unknown): Record<string, number> | null {
  if (!isObject(raw)) return null;
  const result: Record<string, number> = {};
  for (const [key, val] of Object.entries(raw)) {
    if (typeof val !== "number") return null;
    result[key] = val;
  }
  return result;
}

/**
 * Parse a single god-view event entry.
 * Returns `null` for unknown or malformed entries — callers skip nulls rather
 * than rejecting the whole frame.
 */
function parseGodEvent(v: unknown): GodEvent | null {
  if (!isObject(v)) return null;
  if (typeof v.ship !== "string") return null;
  if (typeof v.event !== "string") return null;
  const ship = v.ship;

  switch (v.event) {
    case "tookShield":
    case "tookHull":
      if (typeof v.amount !== "number" || typeof v.by !== "string") return null;
      return { ship, event: v.event, amount: v.amount, by: v.by };
    case "lanceTookHull":
      if (typeof v.amount !== "number" || typeof v.by !== "string") return null;
      return { ship, event: "lanceTookHull", amount: v.amount, by: v.by };
    case "shieldDown":
    case "relicTaken":
    case "cannonFired":
    case "afterburnerExpired":
    case "bulwarkExpired":
    case "respawned":
      return { ship, event: v.event };
    case "collisionTookShield":
    case "collisionTookHull":
      if (typeof v.amount !== "number") return null;
      return { ship, event: v.event, amount: v.amount };
    case "relicDropped": {
      const pos = parseVec2(v.pos);
      if (typeof v.relicId !== "string" || !pos) return null;
      return { ship, event: "relicDropped", relicId: v.relicId, pos };
    }
    case "relicBanked":
      if (typeof v.value !== "number") return null;
      return { ship, event: "relicBanked", value: v.value };
    case "sigilGranted":
    case "sigilDischarged":
      if (typeof v.which !== "string") return null;
      return { ship, event: v.event, which: v.which };
    case "singularityDeployed":
    case "mineDeployed": {
      const pos = parseVec2(v.pos);
      if (typeof v.id !== "string" || !pos) return null;
      return { ship, event: v.event, id: v.id, pos };
    }
    case "mineDetonated": {
      const pos = parseVec2(v.pos);
      if (typeof v.mineId !== "string" || !pos) return null;
      return { ship, event: "mineDetonated", mineId: v.mineId, pos };
    }
    case "killedShip":
      if (typeof v.victim !== "string") return null;
      return { ship, event: "killedShip", victim: v.victim };
    case "died":
      if (v.by !== null && typeof v.by !== "string") return null;
      return { ship, event: "died", by: v.by as string | null };
    default:
      return null; // Unknown tag — skip gracefully
  }
}

// ── Public API ────────────────────────────────────────────────────────────────

/**
 * Parse and validate a raw JSON value received from the observer WS stream
 * into a typed `GodViewFrame`.
 *
 * Returns `null` if the value is missing required fields or has wrong types.
 *
 * Pure function — no side effects, no global state; covered by unit tests.
 *
 * @param raw - The result of `JSON.parse(message)` from the WS socket.
 */
export function parseGodViewFrame(raw: unknown): GodViewFrame | null {
  if (!isObject(raw)) return null;
  if (raw["type"] !== "godView") return null;
  if (typeof raw.tick !== "number") return null;
  if (typeof raw.maxTicks !== "number") return null;
  if (typeof raw.seed !== "number") return null;

  const arena = parseArenaDims(raw.arena);
  if (!arena) return null;

  const ships = parseArray(raw.ships, parseShip);
  if (!ships) return null;

  const anchors = parseArray(raw.anchors, parseAnchor);
  if (!anchors) return null;

  const relics = parseArray(raw.relics, parseRelic);
  if (!relics) return null;

  const asteroids = parseArray(raw.asteroids, parseAsteroid);
  if (!asteroids) return null;

  const projectiles = parseArray(raw.projectiles, parseProjectile);
  if (!projectiles) return null;

  const singularities = parseArray(raw.singularities, parseSingularity);
  if (!singularities) return null;

  const mines = parseArray(raw.mines, parseMine);
  if (!mines) return null;

  const scores = parseScores(raw.scores);
  if (!scores) return null;

  // Events: missing → empty array; malformed entries → skipped (lenient)
  const events: GodEvent[] = [];
  if (Array.isArray(raw.events)) {
    for (const item of raw.events) {
      const ev = parseGodEvent(item);
      if (ev !== null) events.push(ev);
    }
  }

  return {
    type: "godView",
    tick: raw.tick,
    maxTicks: raw.maxTicks,
    seed: raw.seed,
    arena,
    ships,
    anchors,
    relics,
    asteroids,
    projectiles,
    singularities,
    mines,
    scores,
    events,
  };
}
