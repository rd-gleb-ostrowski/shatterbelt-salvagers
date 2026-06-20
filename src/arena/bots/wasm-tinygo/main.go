// Bare-minimal WASM Bot for Shatterbelt Salvagers — TinyGo reference.
//
// Hides nothing about the core-wasm ABI (ADR-0004 / PROTOCOL.md §9). The module
// exports the ABI by hand: `memory` (exported automatically), `alloc`, `init`,
// and `tick`. The host calls `alloc(len)` to get a buffer, writes observation
// JSON into our linear memory, then calls `init`/`tick` with the pointer +
// length. `tick` returns the packed `(out_ptr << 32) | out_len`.
//
// Memory is touched directly via `unsafe`; JSON is parsed with the standard
// `encoding/json`. The per-tick decision is a trivial placeholder.
//
// Build (freestanding wasm, no host imports):
//   tinygo build -target=wasm-unknown -o build/bot.wasm .

package main

import (
	"encoding/json"
	"math"
	"unsafe"
)

func main() {} // required entry point; the Arena calls the exports below

// Keeps allocations referenced so TinyGo's GC does not reclaim them while the
// host still holds the pointer. Leaks for the match's lifetime — fine here.
var keepAlive = map[int32][]byte{}

// The host calls alloc(len) to get a buffer in our memory, then writes the
// observation JSON into it.
//
//go:wasmexport alloc
func alloc(size int32) int32 {
	buf := make([]byte, size)
	ptr := int32(uintptr(unsafe.Pointer(&buf[0])))
	keepAlive[ptr] = buf
	return ptr
}

// Read `length` bytes of JSON at `ptr` directly out of our linear memory.
func readBytes(ptr, length int32) []byte {
	return unsafe.Slice((*byte)(unsafe.Pointer(uintptr(ptr))), length)
}

// Minimal typed view of the observation (§6) — only the fields we use.
type vec struct {
	X float64 `json:"x"`
	Y float64 `json:"y"`
}
type selfView struct {
	Pos     vec     `json:"pos"`
	Heading float64 `json:"heading"`
}
type relic struct {
	Pos vec `json:"pos"`
}
type observation struct {
	Self   selfView `json:"self"`
	Relics []relic  `json:"relics"`
}

// Called once before the match with the tick-0 observation. A real bot could
// precompute here; this reference simply accepts it.
//
//go:wasmexport init
func initBot(ptr, length int32) {
	_ = readBytes(ptr, length)
}

// Holds the most recent action JSON; kept alive past the tick return so the
// host can read it before the next call.
var outBuf []byte

// Called each tick: parse the observation, decide an action, write the action
// JSON into our memory, and return the packed (out_ptr << 32) | out_len.
//
//go:wasmexport tick
func tick(ptr, length int32) int64 {
	var obs observation
	_ = json.Unmarshal(readBytes(ptr, length), &obs)

	// --- trivial placeholder decision: steer at the nearest relic, thrust, fire ---
	turn := 0.0
	if len(obs.Relics) > 0 {
		px, py := obs.Self.Pos.X, obs.Self.Pos.Y
		best := math.MaxFloat64
		var nearest relic
		for _, r := range obs.Relics {
			d := (r.Pos.X-px)*(r.Pos.X-px) + (r.Pos.Y-py)*(r.Pos.Y-py)
			if d < best {
				best = d
				nearest = r
			}
		}
		want := math.Atan2(nearest.Pos.Y-py, nearest.Pos.X-px)
		diff := math.Atan2(math.Sin(want-obs.Self.Heading), math.Cos(want-obs.Self.Heading))
		turn = math.Max(-1, math.Min(1, diff))
	}

	action := map[string]interface{}{
		"type":   "action",
		"turn":   turn,
		"thrust": 1.0,
		"fire":   true,
	}
	outBuf, _ = json.Marshal(action)

	outPtr := int32(uintptr(unsafe.Pointer(&outBuf[0])))
	return (int64(outPtr) << 32) | int64(len(outBuf))
}
