use crate::{
    error::AppError,
    models::{DeploymentStripeAccount, DeploymentStripeAccountDetails},
    state::AppState,
};

use super::Query;

pub struct GetDeploymentStripeAccountQuery {
    deployment_id: i64,
}

impl GetDeploymentStripeAccountQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Query for GetDeploymentStripeAccountQuery {
    type Output = DeploymentStripeAccountDetails;

    async fn execute(&self, app_state: &AppState) -> Result<DeploymentStripeAccountDetails, AppError> {
        let account = sqlx::query_as!(
            DeploymentStripeAccount,
            r#"
            SELECT 
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
            FROM deployment_stripe_accounts 
            WHERE deployment_id = $1
            "#,
            self.deployment_id
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::NotFound("Stripe account not found".to_string()),
            _ => AppError::Internal(e.to_string()),
        })?;

        Ok(account.into())
    }
}

pub struct GetDeploymentStripeAccountByAccountIdQuery {
    stripe_account_id: String,
}

impl GetDeploymentStripeAccountByAccountIdQuery {
    pub fn new(stripe_account_id: String) -> Self {
        Self { stripe_account_id }
    }
}

impl Query for GetDeploymentStripeAccountByAccountIdQuery {
    type Output = DeploymentStripeAccount;

    async fn execute(&self, app_state: &AppState) -> Result<DeploymentStripeAccount, AppError> {
        let account = sqlx::query_as!(
            DeploymentStripeAccount,
            r#"
            SELECT 
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
            FROM deployment_stripe_accounts 
            WHERE stripe_account_id = $1
            "#,
            self.stripe_account_id
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