use crate::{
    error::AppError,
    models::{DeploymentStripeAccount, StripeAccountType},
    state::AppState,
};
use chrono::{DateTime, Utc};
use serde_json::Value;

use super::Command;

pub struct CreateDeploymentStripeAccountCommand {
    pub deployment_id: i64,
    pub stripe_account_id: String,
    pub stripe_user_id: Option<String>,
    pub access_token_encrypted: Option<String>,
    pub refresh_token_encrypted: Option<String>,
    pub account_type: StripeAccountType,
    pub onboarding_url: Option<String>,
    pub metadata: Option<Value>,
}

impl Command for CreateDeploymentStripeAccountCommand {
    type Output = DeploymentStripeAccount;

    async fn execute(self, app_state: &AppState) -> Result<DeploymentStripeAccount, AppError> {
        let account = sqlx::query_as!(
            DeploymentStripeAccount,
            r#"
            INSERT INTO deployment_stripe_accounts (
                deployment_id,
                stripe_account_id,
                stripe_user_id,
                access_token_encrypted,
                refresh_token_encrypted,
                account_type,
                onboarding_url,
                metadata
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING 
                id,
                created_at,
                updated_at,
                deployment_id,
                stripe_account_id,
                stripe_user_id,
                access_token_encrypted,
                refresh_token_encrypted,
                account_type as "account_type!: crate::models::StripeAccountType",
                charges_enabled,
                details_submitted,
                setup_completed_at,
                onboarding_url,
                dashboard_url,
                country,
                default_currency,
                metadata
            "#,
            self.deployment_id,
            self.stripe_account_id,
            self.stripe_user_id,
            self.access_token_encrypted,
            self.refresh_token_encrypted,
            self.account_type as StripeAccountType,
            self.onboarding_url,
            self.metadata
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.constraint() == Some("deployment_stripe_accounts_deployment_id_fkey") => {
                AppError::BadRequest("Invalid deployment ID".to_string())
            }
            sqlx::Error::Database(db_err) if db_err.constraint() == Some("deployment_stripe_accounts_deployment_id_key") => {
                AppError::BadRequest("Stripe account already exists for this deployment".to_string())
            }
            _ => AppError::Internal(e.to_string()),
        })?;

        Ok(account)
    }
}

pub struct UpdateDeploymentStripeAccountCommand {
    pub deployment_id: i64,
    pub charges_enabled: Option<bool>,
    pub details_submitted: Option<bool>,
    pub setup_completed_at: Option<Option<DateTime<Utc>>>,
    pub dashboard_url: Option<Option<String>>,
    pub country: Option<Option<String>>,
    pub default_currency: Option<Option<String>>,
    pub metadata: Option<Option<Value>>,
}

impl Command for UpdateDeploymentStripeAccountCommand {
    type Output = DeploymentStripeAccount;

    async fn execute(self, app_state: &AppState) -> Result<DeploymentStripeAccount, AppError> {
        let account = sqlx::query_as!(
            DeploymentStripeAccount,
            r#"
            UPDATE deployment_stripe_accounts 
            SET 
                updated_at = NOW(),
                charges_enabled = COALESCE($2, charges_enabled),
                details_submitted = COALESCE($3, details_submitted),
                setup_completed_at = COALESCE($4, setup_completed_at),
                dashboard_url = COALESCE($5, dashboard_url),
                country = COALESCE($6, country),
                default_currency = COALESCE($7, default_currency),
                metadata = COALESCE($8, metadata)
            WHERE deployment_id = $1
            RETURNING 
                id,
                created_at,
                updated_at,
                deployment_id,
                stripe_account_id,
                stripe_user_id,
                access_token_encrypted,
                refresh_token_encrypted,
                account_type as "account_type!: crate::models::StripeAccountType",
                charges_enabled,
                details_submitted,
                setup_completed_at,
                onboarding_url,
                dashboard_url,
                country,
                default_currency,
                metadata
            "#,
            self.deployment_id,
            self.charges_enabled,
            self.details_submitted,
            self.setup_completed_at.flatten(),
            self.dashboard_url.flatten(),
            self.country.flatten(),
            self.default_currency.flatten(),
            self.metadata.flatten()
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::NotFound("Stripe account not found".to_string()),
            _ => AppError::Internal(e.to_string()),
        })?;

        Ok(account)
    }
}

pub struct DeleteDeploymentStripeAccountCommand {
    pub deployment_id: i64,
}

impl Command for DeleteDeploymentStripeAccountCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<(), AppError> {
        let result = sqlx::query!(
            "DELETE FROM deployment_stripe_accounts WHERE deployment_id = $1",
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Stripe account not found".to_string()));
        }

        Ok(())
    }
}