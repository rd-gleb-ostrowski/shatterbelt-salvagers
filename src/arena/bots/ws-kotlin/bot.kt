// Bare-minimal WS Bot for Shatterbelt Salvagers — Kotlin / JVM reference.
//
// Hides nothing. The full wire protocol (PROTOCOL.md §4, §5, §6, §8) is written
// out inline: register for a token over HTTP, open a WebSocket, do the
// welcome -> join -> assigned -> matchStart handshake, then each tick parse the
// observation JSON and send back a valid action JSON until matchEnd.
//
// Transport uses ONLY the JDK: java.net.http.HttpClient for the register POST
// and its built-in WebSocket. The only third-party piece is org.json, a tiny
// JSON parser (the JVM has none built in). The per-tick decision is a trivial
// placeholder — the point is to show the bytes on the wire.

import java.net.URI
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import java.net.http.WebSocket
import java.util.concurrent.CompletableFuture
import java.util.concurrent.CompletionStage
import java.util.concurrent.CountDownLatch
import org.json.JSONObject

val HTTP: String = System.getenv("ARENA_HTTP") ?: "http://localhost:3000"
val WS: String = System.getenv("ARENA_WS") ?: "ws://localhost:3000/ws"
val PASSWORD: String = System.getenv("ARENA_PASSWORD") ?: "arena"
val TEAM: String = System.getenv("ARENA_TEAM") ?: "team-kotlin"
// Skip registration and use a pre-issued token if one is supplied.
val PRESET_TOKEN: String? = System.getenv("ARENA_TOKEN")

// POST /register {password, team} -> {token} (PROTOCOL.md §4).
fun register(client: HttpClient): String {
    val body = JSONObject().put("password", PASSWORD).put("team", TEAM).toString()
    val req = HttpRequest.newBuilder(URI.create("$HTTP/register"))
        .header("Content-Type", "application/json")
        .POST(HttpRequest.BodyPublishers.ofString(body))
        .build()
    val res = client.send(req, HttpResponse.BodyHandlers.ofString())
    check(res.statusCode() == 200) { "register failed: ${res.statusCode()}" }
    return JSONObject(res.body()).getString("token")
}

// Turn one observation (§6) into one action JSON string (§8). Trivial
// placeholder: steer at the nearest relic, thrust forward, hold the trigger.
fun decide(obs: JSONObject): String {
    val me = obs.getJSONObject("self")
    val pos = me.getJSONObject("pos")
    val px = pos.getDouble("x")
    val py = pos.getDouble("y")
    val heading = me.getDouble("heading")

    var turn = 0.0
    val relics = obs.optJSONArray("relics")
    if (relics != null && relics.length() > 0) {
        var best = Double.MAX_VALUE
        var nearest = relics.getJSONObject(0)
        for (i in 0 until relics.length()) {
            val rp = relics.getJSONObject(i).getJSONObject("pos")
            val d = Math.pow(rp.getDouble("x") - px, 2.0) + Math.pow(rp.getDouble("y") - py, 2.0)
            if (d < best) { best = d; nearest = relics.getJSONObject(i) }
        }
        val rp = nearest.getJSONObject("pos")
        val want = Math.atan2(rp.getDouble("y") - py, rp.getDouble("x") - px)
        val diff = Math.atan2(Math.sin(want - heading), Math.cos(want - heading))
        turn = diff.coerceIn(-1.0, 1.0)
    }
    return """{"type":"action","turn":$turn,"thrust":1.0,"fire":true}"""
}

// Accumulates (possibly fragmented) text frames and drives the handshake/loop.
class BotListener(private val token: String, private val done: CountDownLatch) : WebSocket.Listener {
    private val buf = StringBuilder()
    private var sessionId = ""
    @Volatile private var finished = false

    override fun onOpen(ws: WebSocket) {
        ws.request(Long.MAX_VALUE) // deliver all frames; no manual re-request
    }

    override fun onText(ws: WebSocket, data: CharSequence, last: Boolean): CompletionStage<*>? {
        buf.append(data)
        if (last) {
            val msg = JSONObject(buf.toString())
            buf.setLength(0)
            when (msg.getString("type")) {
                // 1. welcome -> echo sessionId, present token + a name (2. join).
                "welcome" -> {
                    sessionId = msg.getString("sessionId")
                    val join = JSONObject().put("sessionId", sessionId).put("token", token).put("name", TEAM)
                    ws.sendText(join.toString(), true)
                }
                // 3. assigned -> our ship id for this match.
                "assigned" -> println("[bot] assigned ship ${msg.getString("shipId")}")
                // 4. per tick: parse the observation, send a valid action.
                "tick" -> ws.sendText(decide(msg), true)
                "matchEnd" -> {
                    println("[bot] matchEnd: ${msg.getJSONObject("results")}")
                    finished = true
                    ws.sendClose(WebSocket.NORMAL_CLOSURE, "done")
                    done.countDown()
                }
                // matchStart and anything else need no reply.
            }
        }
        return null
    }

    override fun onError(ws: WebSocket, error: Throwable) {
        // After matchEnd the server closes the socket; ignore the resulting reset.
        if (!finished) System.err.println("[bot] websocket error: ${error.message}")
        done.countDown()
    }

    override fun onClose(ws: WebSocket, statusCode: Int, reason: String): CompletionStage<*>? {
        done.countDown()
        return null
    }
}

fun main() {
    val client = HttpClient.newHttpClient()
    val token = PRESET_TOKEN ?: register(client)
    println("[bot] token acquired; connecting to $WS")

    val done = CountDownLatch(1)
    client.newWebSocketBuilder()
        .buildAsync(URI.create(WS), BotListener(token, done))
        .join()

    done.await() // block until matchEnd / close
    println("[bot] done")
}
