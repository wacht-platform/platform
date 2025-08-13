use common::error::AppError;
use models::{SignIn};
use common::state::AppState;

use super::Query;

pub struct GetSignInQuery {
    signin_id: i64,
}

impl GetSignInQuery {
    pub fn new(signin_id: i64) -> Self {
        Self { signin_id }
    }
}

impl Query for GetSignInQuery {
    type Output = SignIn;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, session_id, user_id, 
                   active_organization_membership_id, active_workspace_membership_id,
                   expires_at, last_active_at, ip_address, browser, device,
                   city, region, region_code, country, country_code
            FROM signins
            WHERE id = $1 AND deleted_at IS NULL
            "#,
            self.signin_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        let signin = SignIn {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            session_id: row.session_id.unwrap_or(0),
            user_id: row.user_id,
            active_organization_membership_id: row.active_organization_membership_id,
            active_workspace_membership_id: row.active_workspace_membership_id,
            expires_at: row.expires_at,
            last_active_at: row.last_active_at,
            ip_address: row.ip_address.unwrap_or_default(),
            browser: row.browser.unwrap_or_default(),
            device: row.device.unwrap_or_default(),
            city: row.city.unwrap_or_default(),
            region: row.region.unwrap_or_default(),
            region_code: row.region_code.unwrap_or_default(),
            country: row.country.unwrap_or_default(),
            country_code: row.country_code.unwrap_or_default(),
        };

        Ok(signin)
    }
}
