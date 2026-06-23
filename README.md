# Shatterbelt Salvagers

Arena bot shooter & collector source code.

[PROTOCOL.md](./src/arena/PROTOCOL.md) gives an overview about what is needed to know to write a bot.

Bot starters (WASM & WebSocket) can be found in [./src/arena/bots](./src/arena/bots/).

To register a team and get a token for WS connection/WASM uploading:

```sh
curl -XPOST https://shatterbelt.echoshell.xyz:18889/register -H 'Content-Type: application/json' -d '{"password":"$PASSWORD","team":"the-rippers"}'
```

WS bots connect to `https://shatterbelt.echoshell.xyz:18889/ws`, WASM bots upload the payload to

```sh
curl -X POST https://shatterbelt.echoshell.xyz:18889/bots -H "Authorization: Bearer $TOKEN_FROM_REGISTER" --data-binary @bot.wasm
```

Potential useful endpoints for bot development:

- `/recordings` - all available match recordings with brief overview
  ```json
  [
  {
    "matchId": "b3b60591-c37b-4159-a262-dc6434b1acb4",
    "seed": 9865,
    "tickCount": 3600,
    "winner": "cat-lovers",// or null
    "scores": [
      [ "dog-lovers", 23.0 ],
      [ "cat-lovers", 26.0 ]
    ]
  }
  ]
  ```
- `/recordings/{id}/download` - a list of all ship intents for each tick
- `/recordings/{id}/download/frames` - a list of all `GodFrames` for the match (what the viewer gets)
  ```json
    [
    {
      "type" "godView",
      "tick": 1,
      "max_ticks": 3600,
      "seed": 5652,
      "arena": {"width": 2000, "height": 2000},
      "ships": [{
        "id": "cat-lovers",
        "class": "skiff",
        "alive": true,
        "invuln": false,
        "pos": {"x": 5.5, "y": 8.9},
        "vel": {"x": 0.5, "y": 0.9},
        "heading": 1.2,
        "ang_vel": 0.9,
        "hull": {"cur": 50, "max": 60},
        "shield": {"cur": 50, "max": 60},
        "aether": {"cur": 50, "max": 60},
        "sigil": "Bulwark", // or null,
        "cannon_cooldown": 14,
        "relics_carried": 2,
        "afterburner_ticks_left": 0,
      }],
      "anchors": [{"ship_id": "cat-lovers", "pos": {"x": 5.5, "y": 8.9},}],
      "relics": [{
        "id": "relic-1",
        "pos": {"x": 5.5, "y": 8.9},
        "vel": {"x": 5.5, "y": 8.9},
        "value": 1,
      }],
      "asteroids": [{
        "id": "asteroid-1",
        "pos": {"x": 5.5, "y": 8.9},
        "vel": {"x": 5.5, "y": 8.9},
        "radius": 1,
      }],
      "projectiles": [{
        "id": "projectile-1",
        "pos": {"x": 5.5, "y": 8.9},
        "vel": {"x": 5.5, "y": 8.9},
        "owner": "cat-lovers",
      }],
      "singularities": [{
        "id": "singularity-1",
        "pos": {"x": 5.5, "y": 8.9},
        "radius": 5,
        "ticksLeft": 10,
      }],
      "mines": [{
        "id": "mine-1",
        "pos": {"x": 5.5, "y": 8.9},
        "own": false,
      }],
      "scores": {"cat-lovers": 42},
      "events": [{
        "ship": "cat-lovers",
        "event": "type",
        //...flattened payload
      }],
    }
    ]
  ```
  ```rust
  // events
  TookShield { amount: f32, by: String },
  TookHull { amount: f32, by: String },
  ShieldDown,
  LanceTookHull { amount: f32, by: String },
  CollisionTookShield { amount: f32 },
  CollisionTookHull { amount: f32 },
  RelicDropped { relic_id: String, pos: Vec2 },
  SigilGranted { which: String },
  SigilDischarged { which: String },
  AfterburnerExpired,
  BulwarkExpired,
  SingularityDeployed { id: String, pos: Vec2 },
  MineDeployed { id: String, pos: Vec2 },
  MineDetonated { mine_id: String, Vec2 },
  KilledShip { victim: String },
  Died { by: Option<String> },
  Respawned,
  CannonFired,
  RelicTaken,
  RelicBanked { value: f32 },
  ```
