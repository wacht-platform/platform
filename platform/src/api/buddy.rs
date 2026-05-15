//! Buddy onboarding-tour state — write-only.
//!
//! Console-only route. The developer using console.wacht.dev reads their
//! own `public_metadata.buddy` via the existing `useUser()` SDK hook; this
//! endpoint exists because `public_metadata` can't be written from the
//! frontend API — only via the backend Wacht SDK. We round-trip through
//! `wacht-rs` (the same SDK customers use) to keep audit, webhook, and
//! cache side-effects consistent.

use std::str::FromStr;

use axum::extract::{Json, State};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use wacht::middleware::extractors::RequireAuth;

use crate::application::response::{ApiErrorResponse, ApiResult};
use common::state::AppState;

/// Per-tour progress entry. Versions match `registry.ts::tours[*].version` —
/// bumping a tour's version invalidates older completion records so the
/// tour replays.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuddyTourSeen {
    pub version: i32,
    pub completed_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuddyState {
    /// Global disable flag — when true, no tours fire at all.
    #[serde(default)]
    pub disabled: bool,
    /// Map of tour_id -> last-seen record.
    #[serde(default)]
    pub tours: std::collections::HashMap<String, BuddyTourSeen>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdateBuddyStateRequest {
    /// When present, replaces `disabled` on the stored state.
    #[serde(default)]
    pub disabled: Option<bool>,
    /// Map of tour_id -> seen entry. Merged into the stored map.
    #[serde(default)]
    pub tours: Option<std::collections::HashMap<String, BuddyTourSeen>>,
    /// When true, all stored tour entries are cleared before merging
    /// `tours` (if any). Powers the user-facing "reset onboarding" affordance.
    #[serde(default)]
    pub reset: bool,
}

/// PATCH /buddy/state — merges into the developer's `public_metadata.buddy`
/// via the Wacht backend SDK. Returns the resulting buddy state so the
/// client can reconcile without a follow-up read.
pub async fn update_buddy_state(
    State(app_state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(request): Json<UpdateBuddyStateRequest>,
) -> ApiResult<BuddyState> {
    let _ = i64::from_str(&auth.user_id).map_err(|_| {
        ApiErrorResponse::bad_request("auth context user_id is not a valid snowflake")
    })?;

    let wacht = app_state
        .wacht_client
        .as_ref()
        .ok_or_else(|| ApiErrorResponse::internal("wacht client not configured"))?;

    let details = wacht
        .users()
        .fetch_user_details(&auth.user_id)
        .send()
        .await
        .map_err(|e| ApiErrorResponse::internal(format!("failed to read user: {e}")))?;

    let mut metadata_obj: Map<String, Value> = match details.public_metadata.clone() {
        Value::Object(obj) => obj,
        Value::Null => Map::new(),
        // Defensive: if someone wrote a non-object to public_metadata we
        // don't want to silently clobber it. Refuse the update.
        _ => {
            return Err(ApiErrorResponse::internal(
                "public_metadata is not a JSON object — refusing to update",
            ));
        }
    };

    let mut buddy = metadata_obj
        .get("buddy")
        .cloned()
        .and_then(|v| serde_json::from_value::<BuddyState>(v).ok())
        .unwrap_or_default();

    if let Some(disabled) = request.disabled {
        buddy.disabled = disabled;
    }
    if request.reset {
        buddy.tours.clear();
    }
    if let Some(updates) = request.tours {
        for (id, entry) in updates {
            buddy.tours.insert(id, entry);
        }
    }

    let buddy_value = serde_json::to_value(&buddy).map_err(|e| {
        ApiErrorResponse::internal(format!("failed to serialize buddy state: {e}"))
    })?;
    metadata_obj.insert("buddy".to_string(), buddy_value);

    let mut update_req = wacht::models::UpdateUserRequest::new();
    update_req.public_metadata = Some(Value::Object(metadata_obj));

    wacht
        .users()
        .update_user(&auth.user_id, update_req)
        .send()
        .await
        .map_err(|e| ApiErrorResponse::internal(format!("failed to update user: {e}")))?;

    Ok(buddy.into())
}
