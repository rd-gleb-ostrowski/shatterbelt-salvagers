// Bare-minimal WASM Bot for Shatterbelt Salvagers — AssemblyScript reference.
//
// Hides nothing about the core-wasm ABI (ADR-0004 / PROTOCOL.md §9). This module
// exports the ABI by hand: `memory` (exported automatically), `alloc`, `init`,
// and `tick`. The host calls `alloc(len)` to get a buffer, writes the
// observation JSON into our linear memory, then calls `init`/`tick` with the
// pointer + length. `tick` returns the packed `(out_ptr << 32) | out_len`.
//
// JSON is parsed with the `assemblyscript-json` library (AssemblyScript has no
// built-in JSON); the ABI plumbing — pointers, lengths, the packed return — is
// written out explicitly. The per-tick decision is a trivial placeholder.

import { JSON } from "assemblyscript-json/assembly";

// Pointer/length of the most recent action JSON. The host reads it right after
// `tick` returns, before the next call, so we free the previous one and keep
// only the latest — no leak.
let outPtr: usize = 0;
let outLen: i32 = 0;

// Read a JSON number field as f64 whether it was encoded as an int or a float.
function num(obj: JSON.Obj, key: string): f64 {
  const v = obj.get(key);
  if (v == null) return 0;
  if (v.isFloat) return (<JSON.Float>v).valueOf();
  if (v.isInteger) return <f64>(<JSON.Integer>v).valueOf();
  return 0;
}

// The host allocates a buffer in our memory and writes observation JSON into it.
export function alloc(len: i32): usize {
  return heap.alloc(len);
}

// Called once before the match with the tick-0 observation. A real bot could
// precompute here; this reference simply accepts it.
export function init(ptr: usize, len: i32): void {
  // intentionally a no-op
}

// Called each tick: parse the observation, decide an action, write the action
// JSON into our memory, and return the packed (out_ptr << 32) | out_len.
export function tick(ptr: usize, len: i32): i64 {
  const str = String.UTF8.decodeUnsafe(ptr, len);
  const obs = <JSON.Obj>JSON.parse(str);

  // --- trivial placeholder decision: steer at the nearest relic, thrust, fire ---
  const me = obs.getObj("self")!;
  const pos = me.getObj("pos")!;
  const px = num(pos, "x");
  const py = num(pos, "y");
  const heading = num(me, "heading");

  let turn: f64 = 0;
  const relics = obs.getArr("relics");
  if (relics != null) {
    const arr = relics.valueOf();
    let bestD = f64.MAX_VALUE;
    let bestIdx = -1;
    for (let i = 0; i < arr.length; i++) {
      const rp = (<JSON.Obj>arr[i]).getObj("pos")!;
      const dx = num(rp, "x") - px;
      const dy = num(rp, "y") - py;
      const d = dx * dx + dy * dy;
      if (d < bestD) { bestD = d; bestIdx = i; }
    }
    if (bestIdx >= 0) {
      const rp = (<JSON.Obj>arr[bestIdx]).getObj("pos")!;
      const want = Math.atan2(num(rp, "y") - py, num(rp, "x") - px);
      const diff = Math.atan2(Math.sin(want - heading), Math.cos(want - heading));
      turn = Math.max(-1, Math.min(1, diff));
    }
  }

  const action =
    '{"type":"action","turn":' + turn.toString() + ',"thrust":1,"fire":true}';

  // Encode into a fresh buffer; free the previous tick's buffer first.
  const buf = String.UTF8.encode(action);
  if (outPtr != 0) heap.free(outPtr);
  outLen = buf.byteLength;
  outPtr = heap.alloc(outLen);
  memory.copy(outPtr, changetype<usize>(buf), outLen);

  return ((<i64>outPtr) << 32) | (<i64>outLen);
}
