use common::error::AppError;

pub struct CreateBillingAccountCommand {
    id: i64,
    owner_id: String,
    owner_type: String,
    legal_name: String,
    billing_email: String,
    billing_phone: Option<String>,
    tax_id: Option<String>,
    address_line1: Option<String>,
    address_line2: Option<String>,
    city: Option<String>,
    state: Option<String>,
    postal_code: Option<String>,
    country: Option<String>,
    max_projects_per_account: Option<i64>,
    max_staging_deployments_per_project: Option<i64>,
}

impl CreateBillingAccountCommand {
    pub fn new(
        id: i64,
        owner_id: String,
        owner_type: String,
        legal_name: String,
        billing_email: String,
    ) -> Self {
        Self {
            id,
            owner_id,
            owner_type,
            legal_name,
            billing_email,
            billing_phone: None,
            tax_id: None,
            address_line1: None,
            address_line2: None,
            city: None,
            state: None,
            postal_code: None,
            country: None,
            max_projects_per_account: None,
            max_staging_deployments_per_project: None,
        }
    }

    pub fn with_billing_phone(mut self, billing_phone: Option<String>) -> Self {
        self.billing_phone = billing_phone;
        self
    }

    pub fn with_tax_id(mut self, tax_id: Option<String>) -> Self {
        self.tax_id = tax_id;
        self
    }

    pub fn with_address_line1(mut self, address_line1: Option<String>) -> Self {
        self.address_line1 = address_line1;
        self
    }

    pub fn with_address_line2(mut self, address_line2: Option<String>) -> Self {
        self.address_line2 = address_line2;
        self
    }

    pub fn with_city(mut self, city: Option<String>) -> Self {
        self.city = city;
        self
    }

    pub fn with_state(mut self, state: Option<String>) -> Self {
        self.state = state;
        self
    }

    pub fn with_postal_code(mut self, postal_code: Option<String>) -> Self {
        self.postal_code = postal_code;
        self
    }

    pub fn with_country(mut self, country: Option<String>) -> Self {
        self.country = country;
        self
    }

    pub fn with_max_projects_per_account(mut self, limit: Option<i64>) -> Self {
        self.max_projects_per_account = limit;
        self
    }

    pub fn with_max_staging_deployments_per_project(mut self, limit: Option<i64>) -> Self {
        self.max_staging_deployments_per_project = limit;
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<i64, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let max_projects_per_account = self
            .max_projects_per_account
            .map(i32::try_from)
            .transpose()
            .map_err(|_| {
                AppError::Validation("max_projects_per_account is out of range".to_string())
            })?;
        let max_staging_deployments_per_project = self
            .max_staging_deployments_per_project
            .map(i32::try_from)
            .transpose()
            .map_err(|_| {
                AppError::Validation(
                    "max_staging_deployments_per_project is out of range".to_string(),
                )
            })?;

        sqlx::query!(
            r#"
            INSERT INTO billing_accounts (
                id,
                owner_id,
                owner_type,
                legal_name,
                billing_email,
                billing_phone,
                tax_id,
                address_line1,
                address_line2,
                city,
                state,
                postal_code,
                country,
                status,
                currency,
                locale,
                pulse_balance_cents,
                max_projects_per_account,
                max_staging_deployments_per_project,
                pulse_usage_disabled,
                pulse_notified_below_five,
                pulse_notified_below_zero,
                pulse_notified_disabled,
                created_at,
                updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 'pending', 'USD', 'en-US', 0, COALESCE($14, 10), COALESCE($15, 3), true, false, false, false, NOW(), NOW())
            "#,
            self.id,
            self.owner_id,
            self.owner_type,
            self.legal_name,
            self.billing_email,
            self.billing_phone,
            self.tax_id,
            self.address_line1.as_deref().unwrap_or(""),
            self.address_line2,
            self.city.as_deref().unwrap_or(""),
            self.state,
            self.postal_code.as_deref().unwrap_or(""),
            self.country.as_deref().unwrap_or("US"),
            max_projects_per_account,
            max_staging_deployments_per_project
        )
        .execute(executor)
        .await?;

        Ok(self.id)
    }
}

pub struct UpdateBillingAccountCommand {
    pub id: i64,
    pub legal_name: Option<String>,
    pub billing_email: Option<String>,
    pub billing_phone: Option<String>,
    pub tax_id: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub max_projects_per_account: Option<i64>,
    pub max_staging_deployments_per_project: Option<i64>,
}

impl UpdateBillingAccountCommand {
    pub fn new(id: i64) -> Self {
        Self {
            id,
            legal_name: None,
            billing_email: None,
            billing_phone: None,
            tax_id: None,
            address_line1: None,
            address_line2: None,
            city: None,
            state: None,
            postal_code: None,
            country: None,
            max_projects_per_account: None,
            max_staging_deployments_per_project: None,
        }
    }

    pub fn with_legal_name(mut self, legal_name: Option<String>) -> Self {
        self.legal_name = legal_name;
        self
    }

    pub fn with_billing_email(mut self, billing_email: Option<String>) -> Self {
        self.billing_email = billing_email;
        self
    }

    pub fn with_billing_phone(mut self, billing_phone: Option<String>) -> Self {
        self.billing_phone = billing_phone;
        self
    }

    pub fn with_tax_id(mut self, tax_id: Option<String>) -> Self {
        self.tax_id = tax_id;
        self
    }

    pub fn with_address_line1(mut self, address_line1: Option<String>) -> Self {
        self.address_line1 = address_line1;
        self
    }

    pub fn with_address_line2(mut self, address_line2: Option<String>) -> Self {
        self.address_line2 = address_line2;
        self
    }

    pub fn with_city(mut self, city: Option<String>) -> Self {
        self.city = city;
        self
    }

    pub fn with_state(mut self, state: Option<String>) -> Self {
        self.state = state;
        self
    }

    pub fn with_postal_code(mut self, postal_code: Option<String>) -> Self {
        self.postal_code = postal_code;
        self
    }

    pub fn with_country(mut self, country: Option<String>) -> Self {
        self.country = country;
        self
    }

    pub fn with_max_projects_per_account(mut self, limit: Option<i64>) -> Self {
        self.max_projects_per_account = limit;
        self
    }

    pub fn with_max_staging_deployments_per_project(mut self, limit: Option<i64>) -> Self {
        self.max_staging_deployments_per_project = limit;
        self
    }
}

pub struct UpdateBillingAccountFromWebhookCommand {
    pub owner_id: String,
    pub legal_name: Option<String>,
    pub billing_email: Option<String>,
    pub billing_phone: Option<String>,
    pub company: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
}

impl UpdateBillingAccountFromWebhookCommand {
    pub fn new(owner_id: String) -> Self {
        Self {
            owner_id,
            legal_name: None,
            billing_email: None,
            billing_phone: None,
            company: None,
            address_line1: None,
            address_line2: None,
            city: None,
            state: None,
            postal_code: None,
            country: None,
        }
    }
}

impl UpdateBillingAccountCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let max_projects_per_account = self
            .max_projects_per_account
            .map(i32::try_from)
            .transpose()
            .map_err(|_| {
                AppError::Validation("max_projects_per_account is out of range".to_string())
            })?;
        let max_staging_deployments_per_project = self
            .max_staging_deployments_per_project
            .map(i32::try_from)
            .transpose()
            .map_err(|_| {
                AppError::Validation(
                    "max_staging_deployments_per_project is out of range".to_string(),
                )
            })?;

        sqlx::query(
            r#"
            UPDATE billing_accounts
            SET
                legal_name = COALESCE($1, legal_name),
                billing_email = COALESCE($2, billing_email),
                billing_phone = COALESCE($3, billing_phone),
                tax_id = COALESCE($4, tax_id),
                address_line1 = COALESCE($5, address_line1),
                address_line2 = COALESCE($6, address_line2),
                city = COALESCE($7, city),
                state = COALESCE($8, state),
                postal_code = COALESCE($9, postal_code),
                country = COALESCE($10, country),
                max_projects_per_account = COALESCE($11, max_projects_per_account),
                max_staging_deployments_per_project = COALESCE($12, max_staging_deployments_per_project),
                updated_at = NOW()
            WHERE id = $13
            "#,
        )
        .bind(self.legal_name)
        .bind(self.billing_email)
        .bind(self.billing_phone)
        .bind(self.tax_id)
        .bind(self.address_line1)
        .bind(self.address_line2)
        .bind(self.city)
        .bind(self.state)
        .bind(self.postal_code)
        .bind(self.country)
        .bind(max_projects_per_account)
        .bind(max_staging_deployments_per_project)
        .bind(self.id)
        .execute(executor)
        .await?;

        Ok(())
    }
}

pub struct UpdateBillingAccountStatusCommand {
    pub owner_id: String,
    pub status: String,
}

impl UpdateBillingAccountStatusCommand {
    pub fn new(owner_id: String, status: String) -> Self {
        Self { owner_id, status }
    }
}

fn normalize_billing_account_status(status: &str) -> &'static str {
    match status.to_ascii_lowercase().as_str() {
        "active" => "active",
        "pending" => "pending",
        "cancelled" | "expired" => "cancelled",
        "failed" | "on_hold" | "payment_failed" => "failed",
        _ => "pending",
    }
}

impl UpdateBillingAccountStatusCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let normalized_status = normalize_billing_account_status(&self.status);

        sqlx::query!(
            r#"
            UPDATE billing_accounts
            SET status = $1, updated_at = NOW()
            WHERE owner_id = $2
            "#,
            normalized_status,
            self.owner_id,
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

pub struct SetProviderCustomerIdCommand {
    pub owner_id: String,
    pub provider_customer_id: String,
}

impl SetProviderCustomerIdCommand {
    pub fn new(owner_id: String, provider_customer_id: String) -> Self {
        Self {
            owner_id,
            provider_customer_id,
        }
    }
}

impl SetProviderCustomerIdCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE billing_accounts
            SET provider_customer_id = $1, updated_at = NOW()
            WHERE owner_id = $2
            "#,
            self.provider_customer_id,
            self.owner_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

pub struct MarkCheckoutSessionCreatedCommand {
    pub owner_id: String,
    pub checkout_session_id: String,
}

impl MarkCheckoutSessionCreatedCommand {
    pub fn new(owner_id: String, checkout_session_id: String) -> Self {
        Self {
            owner_id,
            checkout_session_id,
        }
    }
}

impl MarkCheckoutSessionCreatedCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE billing_accounts
            SET
                last_checkout_session_created_at = NOW(),
                last_checkout_session_id = $1,
                checkout_flow_state = 'checkout_created',
                last_payment_succeeded_at = NULL,
                last_subscription_activated_at = NULL,
                last_billing_webhook_event = NULL,
                checkout_flow_error = NULL,
                updated_at = NOW()
            WHERE owner_id = $2
            "#,
            self.checkout_session_id,
            self.owner_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

pub struct MarkPaymentSucceededCommand {
    pub owner_id: String,
    pub webhook_event: String,
}

impl MarkPaymentSucceededCommand {
    pub fn new(owner_id: String, webhook_event: String) -> Self {
        Self {
            owner_id,
            webhook_event,
        }
    }
}

impl MarkPaymentSucceededCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE billing_accounts
            SET
                last_payment_succeeded_at = NOW(),
                last_billing_webhook_event = $1,
                checkout_flow_state = CASE
                    WHEN last_subscription_activated_at IS NOT NULL THEN 'active'
                    ELSE 'payment_received_waiting_subscription'
                END,
                checkout_flow_error = NULL,
                updated_at = NOW()
            WHERE owner_id = $2
            "#,
            self.webhook_event,
            self.owner_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

pub struct MarkSubscriptionActivatedCommand {
    pub owner_id: String,
    pub webhook_event: String,
}

impl MarkSubscriptionActivatedCommand {
    pub fn new(owner_id: String, webhook_event: String) -> Self {
        Self {
            owner_id,
            webhook_event,
        }
    }
}

impl MarkSubscriptionActivatedCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE billing_accounts
            SET
                last_subscription_activated_at = NOW(),
                last_billing_webhook_event = $1,
                checkout_flow_state = CASE
                    WHEN last_payment_succeeded_at IS NOT NULL THEN 'active'
                    ELSE 'subscription_active_waiting_payment'
                END,
                checkout_flow_error = NULL,
                updated_at = NOW()
            WHERE owner_id = $2
            "#,
            self.webhook_event,
            self.owner_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

pub struct MarkCheckoutFlowFailedCommand {
    pub owner_id: String,
    pub webhook_event: String,
    pub reason: String,
}

impl MarkCheckoutFlowFailedCommand {
    pub fn new(owner_id: String, webhook_event: String, reason: String) -> Self {
        Self {
            owner_id,
            webhook_event,
            reason,
        }
    }
}

impl MarkCheckoutFlowFailedCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE billing_accounts
            SET
                checkout_flow_state = 'failed',
                last_billing_webhook_event = $1,
                checkout_flow_error = $2,
                updated_at = NOW()
            WHERE owner_id = $3
            "#,
            self.webhook_event,
            self.reason,
            self.owner_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl UpdateBillingAccountFromWebhookCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        // Simple approach: just update all fields that are provided
        // Use COALESCE to keep existing values if new value is NULL
        sqlx::query!(
            r#"
            UPDATE billing_accounts 
            SET 
                legal_name = COALESCE($1, legal_name),
                billing_email = COALESCE($2, billing_email),
                billing_phone = COALESCE($3, billing_phone),
                address_line1 = COALESCE($4, address_line1),
                address_line2 = COALESCE($5, address_line2),
                city = COALESCE($6, city),
                state = COALESCE($7, state),
                postal_code = COALESCE($8, postal_code),
                country = COALESCE($9, country),
                updated_at = NOW()
            WHERE owner_id = $10
            "#,
            self.legal_name,
            self.billing_email,
            self.billing_phone,
            self.address_line1,
            self.address_line2,
            self.city,
            self.state,
            self.postal_code,
            self.country,
            self.owner_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}
