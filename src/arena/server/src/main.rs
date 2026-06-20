//! Binary entry point for the Arena server.
//!
//! ## Environment variables
//!
//! | Variable                    | Default                      | Description                                              |
//! |-----------------------------|------------------------------|----------------------------------------------------------|
//! | `ARENA_PORT`                | `3000`                       | TCP port to bind.                                        |
//! | `ARENA_EVENT_PASSWORD`      | `arena`                      | Pre-shared event password for `POST /register`.          |
//! | `ARENA_FACILITATOR_PASSWORD`| `facilitator`                | Pre-shared facilitator password for admin endpoints.     |
//! | `ARENA_STATIC_DIR`          | `src/arena/frontend/dist`    | Path to the built frontend `dist/` directory.            |
//! | `ARENA_DATA_DIR`            | `arena-data/recordings`      | Directory for durable recording storage.                 |
//!
//! ## Quick start
//!
//! ```sh
//! cargo run -p arena-server
//! # Viewer: http://localhost:3000/
//! # Admin:  http://localhost:3000/admin.html
//! ```
//!
//! Start a match via the Admin UI or:
//! ```sh
//! curl -X POST http://localhost:3000/admin/matches \
//!      -H "Authorization: Facilitator facilitator" \
//!      -H "Content-Type: application/json" \
//!      -d '{"seed":42}'
//! ```

use std::path::PathBuf;

use arena_server::recording::RecordingStore;
use arena_server::routes::build_app_with_store;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("ARENA_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);

    let event_password =
        std::env::var("ARENA_EVENT_PASSWORD").unwrap_or_else(|_| "arena".to_string());

    let facilitator_password =
        std::env::var("ARENA_FACILITATOR_PASSWORD").unwrap_or_else(|_| "facilitator".to_string());

    let static_dir = std::env::var("ARENA_STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("src/arena/frontend/dist"));

    let data_dir = std::env::var("ARENA_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("arena-data/recordings"));

    let recording_store = RecordingStore::with_dir(data_dir);

    let router =
        build_app_with_store(event_password, facilitator_password, Some(static_dir), recording_store);

    let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[arena-server] ERROR: failed to bind 0.0.0.0:{port} — {e}");
            std::process::exit(1);
        }
    };

    let addr = listener.local_addr().expect("listener has a local address");
    println!("[arena-server] listening on http://0.0.0.0:{}", addr.port());
    println!("[arena-server] Viewer : http://localhost:{}/", addr.port());
    println!("[arena-server] Admin  : http://localhost:{}/admin.html", addr.port());
    println!(
        "[arena-server] To start a match: \
         POST /admin/matches  Authorization: Facilitator <ARENA_FACILITATOR_PASSWORD>"
    );

    if let Err(e) = axum::serve(listener, router).await {
        eprintln!("[arena-server] ERROR: server stopped — {e}");
        std::process::exit(1);
    }
}
