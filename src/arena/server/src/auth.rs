//! Token registry for participant registration (issue 02).
//!
//! Pre-shared event password → team token. No accounts.
//!
//! # Design
//!
//! [`TokenRegistry`] is `Send + Sync` and cheap to clone as `Arc<TokenRegistry>`.
//! All handlers and, later, the WS-join (issue 03) and WASM-upload (issue 04)
//! paths share one registry instance via the [`crate::routes::AppState`].
//!
//! # Re-registration
//!
//! A team that re-registers receives a **new** token; the previous token is
//! **revoked**. This lets a team recover after losing their token without leaving
//! stale credentials floating around.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use uuid::Uuid;

// ── TokenRegistry ─────────────────────────────────────────────────────────────

/// Issues and resolves per-team tokens.
///
/// Create one instance at server startup with [`TokenRegistry::new`] and share
/// it across all handlers as `Arc<TokenRegistry>`.
///
/// ## Seams for future issues
///
/// | Future issue | Usage |
/// |---|---|
/// | 03 (WS join) | `resolve(token)` in the `join` handshake to identify the team |
/// | 04 (WASM upload) | `resolve(token)` from the `Authorization` header of `POST /bots` |
/// | 09 (connection resolver) | look up the team's slot via the resolved identity |
#[derive(Debug, Default)]
pub struct TokenRegistry {
    /// token → team identity
    tokens: RwLock<HashMap<String, String>>,
    /// team identity → current token (needed to revoke on re-registration)
    team_to_token: RwLock<HashMap<String, String>>,
}

impl TokenRegistry {
    /// Create a new, empty registry wrapped in `Arc`.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Issue a fresh token for `team`, revoking any previous token for that team.
    ///
    /// Returns the new token string. The returned value is what `POST /register`
    /// hands back to the participant.
    pub fn register(&self, team: &str) -> String {
        let token = Uuid::new_v4().to_string();

        // Acquire both locks together to keep the two maps consistent.
        let mut tokens = self.tokens.write().unwrap();
        let mut team_to_token = self.team_to_token.write().unwrap();

        // Revoke the previous token for this team, if any.
        if let Some(old) = team_to_token.get(team) {
            tokens.remove(old);
        }

        tokens.insert(token.clone(), team.to_owned());
        team_to_token.insert(team.to_owned(), token.clone());

        token
    }

    /// Resolve a `token` to the team identity it was issued for.
    ///
    /// Returns `None` if the token is unknown or has been revoked.
    pub fn resolve(&self, token: &str) -> Option<String> {
        self.tokens.read().unwrap().get(token).cloned()
    }

    pub fn registered_teams(&self) -> Vec<String> {
        self.team_to_token.read().unwrap().keys().cloned().collect()
    }
}
