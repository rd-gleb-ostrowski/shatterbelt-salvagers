//! Integration tests for the runnable server binary (A1 + A2).
//!
//! These tests boot the **real** axum server on an ephemeral TCP port, then
//! make genuine HTTP requests with [`reqwest`].  They verify:
//!
//! 1. The server binds and responds to a basic request.
//! 2. An API route works over real HTTP (`GET /ladder/standings` → 200 JSON).
//! 3. A facilitator-gated route is still protected (`GET /admin/bots` without
//!    auth → 401) — static serving must NOT shadow API routes.
//! 4. With a temp static dir, `GET /` returns the stub `index.html` and an
//!    unknown path falls back to `index.html` (SPA catch-all).
//! 5. `GET /admin/bots` still returns 401 when the static dir is present.

use std::path::PathBuf;
use std::time::Duration;

use arena_server::routes::build_app;

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Bind an ephemeral port, spawn the server in a background task, and return
/// the base URL string.  The `JoinHandle` keeps the server alive for the
/// duration of the test; drop it (via `_handle`) to let it be cleaned up.
async fn start_server(static_dir: Option<PathBuf>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral bind");
    let port = listener.local_addr().unwrap().port();
    let router = build_app(
        "test-event".to_string(),
        "test-facilitator".to_string(),
        static_dir,
    );
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("server error in test");
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

/// Tiny reqwest client with a short timeout so tests never hang.
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

// ── Tests ──────────────────────────────────────────────────────────────────────

/// (1) The server binds and responds — any request returns a non-connection-error
///     response.
#[tokio::test]
async fn test_server_binds_and_responds() {
    let (base, _handle) = start_server(None).await;
    let resp = client()
        .get(format!("{base}/ladder/standings"))
        .send()
        .await
        .expect("request should succeed");
    assert!(
        resp.status().is_success(),
        "expected 2xx, got {}",
        resp.status()
    );
}

/// (2) `GET /ladder/standings` returns 200 with a JSON array over a real socket.
#[tokio::test]
async fn test_api_ladder_standings_real_http() {
    let (base, _handle) = start_server(None).await;
    let resp = client()
        .get(format!("{base}/ladder/standings"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.is_array(), "expected JSON array, got {body}");
}

/// (3) `GET /admin/bots` without auth → 401 (no static dir present).
#[tokio::test]
async fn test_admin_bots_gated_without_static_dir() {
    let (base, _handle) = start_server(None).await;
    let resp = client()
        .get(format!("{base}/admin/bots"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "GET /admin/bots without auth should be 401"
    );
}

/// (4) With a temp static dir containing stub HTML files:
///     - `GET /` returns the stub `index.html` content (200).
///     - `GET /some-unknown-spa-path` falls back to `index.html` (200, SPA catch-all).
#[tokio::test]
async fn test_static_serving_and_spa_fallback() {
    // Build a minimal temp static dir with index.html and admin.html.
    let tmp = tempdir_with_stubs();
    let (base, _handle) = start_server(Some(tmp.path().to_path_buf())).await;

    // Root → index.html
    let resp = client().get(format!("{base}/")).send().await.unwrap();
    assert_eq!(resp.status(), 200, "GET / should return 200");
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("stub-viewer"),
        "GET / should serve the stub index.html, got: {body}"
    );

    // Unknown path → SPA fallback to index.html (200)
    let resp = client()
        .get(format!("{base}/totally/unknown/path"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "unknown SPA path should fall back to index.html with 200");
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("stub-viewer"),
        "SPA fallback should serve index.html, got: {body}"
    );
}

/// (5) `GET /admin/bots` still returns 401 even when the static dir is present
///     — API routes take precedence over static serving fallback.
#[tokio::test]
async fn test_admin_api_not_shadowed_by_static_serving() {
    let tmp = tempdir_with_stubs();
    let (base, _handle) = start_server(Some(tmp.path().to_path_buf())).await;

    let resp = client()
        .get(format!("{base}/admin/bots"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "GET /admin/bots without auth must still return 401 even with static dir present"
    );
}

/// Bonus: `GET /admin` (convenience redirect) → `admin.html` content.
#[tokio::test]
async fn test_admin_convenience_redirect() {
    let tmp = tempdir_with_stubs();
    let (base, _handle) = start_server(Some(tmp.path().to_path_buf())).await;

    // follow_redirects is on by default in reqwest
    let resp = client().get(format!("{base}/admin")).send().await.unwrap();
    assert_eq!(resp.status(), 200, "GET /admin should redirect to admin.html → 200");
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("stub-admin"),
        "GET /admin should eventually serve admin.html, got: {body}"
    );
}

/// Bonus: missing static dir prints a warning but the server still serves the
///        API normally.
#[tokio::test]
async fn test_missing_static_dir_serves_api() {
    let (base, _handle) =
        start_server(Some(PathBuf::from("/nonexistent/path/that/does/not/exist"))).await;

    // API still works
    let resp = client()
        .get(format!("{base}/ladder/standings"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Auth routes still gated
    let resp = client()
        .get(format!("{base}/admin/bots"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

// ── Helpers ────────────────────────────────────────────────────────────────────

/// A small wrapper around a temp directory that keeps it alive for the test.
struct TempDir {
    inner: std::sync::Arc<tempfile::TempDir>,
}

impl TempDir {
    fn path(&self) -> &std::path::Path {
        self.inner.path()
    }
}

/// Create a temp dir with stub `index.html` and `admin.html`.
fn tempdir_with_stubs() -> TempDir {
    let dir = tempfile::Builder::new()
        .prefix("arena-server-test-")
        .tempdir()
        .expect("tempdir");
    std::fs::write(
        dir.path().join("index.html"),
        "<!DOCTYPE html><html><body>stub-viewer</body></html>",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("admin.html"),
        "<!DOCTYPE html><html><body>stub-admin</body></html>",
    )
    .unwrap();
    TempDir {
        inner: std::sync::Arc::new(dir),
    }
}
