#!/usr/bin/env python3
"""Bare-minimal WS Bot for Shatterbelt Salvagers — Python reference.

Hides nothing. The full wire protocol (PROTOCOL.md §4, §5, §6, §8) is written
out inline: register for a token, open a WebSocket, do the
welcome -> join -> assigned -> matchStart handshake, then each tick parse the
observation JSON and send back a valid action JSON until matchEnd.

This is a REFERENCE, not a strategy: the per-tick decision is deliberately
trivial. The point is to show exactly what bytes go on the wire.

Run:
    ARENA_HTTP=http://localhost:3000 ARENA_WS=ws://localhost:3000/ws \
    ARENA_PASSWORD=arena ARENA_TEAM=team-py python3 bot.py

Dependencies: the `websockets` library (pip install websockets). Everything
else is the Python standard library.
"""
import json
import os
import urllib.request

from websockets.sync.client import connect

HTTP = os.environ.get("ARENA_HTTP", "http://localhost:3000")
WS = os.environ.get("ARENA_WS", "ws://localhost:3000/ws")
PASSWORD = os.environ.get("ARENA_PASSWORD", "arena")
TEAM = os.environ.get("ARENA_TEAM", "team-py")
# Skip registration and use a pre-issued token if one is supplied.
TOKEN = os.environ.get("ARENA_TOKEN")


def register() -> str:
    """POST /register {password, team} -> {token} (PROTOCOL.md §4)."""
    body = json.dumps({"password": PASSWORD, "team": TEAM}).encode()
    req = urllib.request.Request(
        HTTP + "/register", data=body, headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read())["token"]


def decide(obs: dict) -> dict:
    """Turn one observation (§6) into one action (§8).

    Trivial placeholder: steer toward the nearest relic, thrust forward, and
    hold the trigger. Swap this body for a real strategy — the wire handling
    around it does not change.
    """
    me = obs["self"]
    action = {"type": "action", "thrust": 1.0, "turn": 0.0, "fire": True}

    relics = obs.get("relics", [])
    if relics:
        # Nearest relic by squared distance — plain arithmetic, no library.
        px, py = me["pos"]["x"], me["pos"]["y"]
        nearest = min(
            relics,
            key=lambda r: (r["pos"]["x"] - px) ** 2 + (r["pos"]["y"] - py) ** 2,
        )
        # Angle to the relic vs. our heading; turn toward it (+ = CCW).
        import math

        want = math.atan2(nearest["pos"]["y"] - py, nearest["pos"]["x"] - px)
        diff = math.atan2(math.sin(want - me["heading"]), math.cos(want - me["heading"]))
        action["turn"] = max(-1.0, min(1.0, diff))

    return action


def main() -> None:
    token = TOKEN or register()
    print(f"[bot] token acquired; connecting to {WS}")

    with connect(WS) as ws:
        # 1. welcome (Arena -> bot): carries the sessionId we must echo back.
        welcome = json.loads(ws.recv())
        assert welcome["type"] == "welcome", welcome
        session_id = welcome["sessionId"]

        # 2. join (bot -> Arena): echo sessionId + present our token + a name.
        ws.send(json.dumps({"sessionId": session_id, "token": token, "name": TEAM}))

        # 3. assigned (Arena -> bot): our ship id for the connection.
        assigned = json.loads(ws.recv())
        assert assigned["type"] == "assigned", assigned
        print(f"[bot] assigned ship {assigned['shipId']}")

        # 4. matchStart, then the per-tick observation/action loop until matchEnd.
        for raw in ws:
            msg = json.loads(raw)
            kind = msg.get("type")
            if kind == "tick":
                ws.send(json.dumps(decide(msg)))
            elif kind == "matchStart":
                print(f"[bot] matchStart")
            elif kind == "matchEnd":
                print(f"[bot] matchEnd: {msg['results']}")

    print("[bot] done")


if __name__ == "__main__":
    main()
