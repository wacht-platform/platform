use common::error::AppError;

pub struct CreateBillingAccountCommand {
    pub owner_id: String,
    pub owner_type: String,
    pub legal_name: String,
    pub billing_email: String,
    pub billing_phone: Option<String>,
    pub tax_id: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
}

impl CreateBillingAccountCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A, id: i64) -> Result<i64, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
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
                pulse_usage_disabled,
                pulse_notified_below_five,
                pulse_notified_below_zero,
                pulse_notified_disabled,
                created_at,
                updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 'pending', 'USD', 'en-US', 0, true, false, false, false, NOW(), NOW())
            "#,
            id,
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
            self.country.as_deref().unwrap_or("US")
        )
        .execute(&mut *conn)
        .await?;

        Ok(id)
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

impl UpdateBillingAccountCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let mut query = String::from("UPDATE billing_accounts SET updated_at = NOW()");
        let mut param_count = 1;

        if self.legal_name.is_some() {
            query.push_str(&format!(", legal_name = ${}", param_count));
            param_count += 1;
        }

        if self.billing_email.is_some() {
            query.push_str(&format!(", billing_email = ${}", param_count));
            param_count += 1;
        }

        if self.billing_phone.is_some() {
            query.push_str(&format!(", billing_phone = ${}", param_count));
            param_count += 1;
        }

        if self.tax_id.is_some() {
            query.push_str(&format!(", tax_id = ${}", param_count));
            param_count += 1;
        }

        if self.address_line1.is_some() {
            query.push_str(&format!(", address_line1 = ${}", param_count));
            param_count += 1;
        }

        if self.address_line2.is_some() {
            query.push_str(&format!(", address_line2 = ${}", param_count));
            param_count += 1;
        }

        if self.city.is_some() {
            query.push_str(&format!(", city = ${}", param_count));
            param_count += 1;
        }

        if self.state.is_some() {
            query.push_str(&format!(", state = ${}", param_count));
            param_count += 1;
        }

        if self.postal_code.is_some() {
            query.push_str(&format!(", postal_code = ${}", param_count));
            param_count += 1;
        }

        if self.country.is_some() {
            query.push_str(&format!(", country = ${}", param_count));
            param_count += 1;
        }

        query.push_str(&format!(" WHERE id = ${}", param_count));

        let mut q = sqlx::query(&query);

        // Bind all parameters in the same order they were added to the query
        if let Some(legal_name) = self.legal_name {
            q = q.bind(legal_name);
        }
        if let Some(billing_email) = self.billing_email {
            q = q.bind(billing_email);
        }
        if let Some(billing_phone) = self.billing_phone {
            q = q.bind(billing_phone);
        }
        if let Some(tax_id) = self.tax_id {
            q = q.bind(tax_id);
        }
        if let Some(address_line1) = self.address_line1 {
            q = q.bind(address_line1);
        }
        if let Some(address_line2) = self.address_line2 {
            q = q.bind(address_line2);
        }
        if let Some(city) = self.city {
            q = q.bind(city);
        }
        if let Some(state) = self.state {
            q = q.bind(state);
        }
        if let Some(postal_code) = self.postal_code {
            q = q.bind(postal_code);
        }
        if let Some(country) = self.country {
            q = q.bind(country);
        }

        // Finally bind the id for the WHERE clause
        q = q.bind(self.id);

        q.execute(&mut *conn).await?;

        Ok(())
    }
}

pub struct UpdateBillingAccountStatusCommand {
    pub owner_id: String,
    pub status: String,
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
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let normalized_status = normalize_billing_account_status(&self.status);

        sqlx::query!(
            r#"
            UPDATE billing_accounts
            SET status = $1, updated_at = NOW()
            WHERE owner_id = $2
            "#,
            normalized_status,
            self.owner_id
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}

pub struct SetProviderCustomerIdCommand {
    pub owner_id: String,
    pub provider_customer_id: String,
}

impl SetProviderCustomerIdCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        sqlx::query!(
            r#"
            UPDATE billing_accounts
            SET provider_customer_id = $1, updated_at = NOW()
            WHERE owner_id = $2
            "#,
            self.provider_customer_id,
            self.owner_id
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}

pub struct MarkCheckoutSessionCreatedCommand {
    pub owner_id: String,
    pub checkout_session_id: String,
}

impl MarkCheckoutSessionCreatedCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
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
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}

pub struct MarkPaymentSucceededCommand {
    pub owner_id: String,
    pub webhook_event: String,
}

impl MarkPaymentSucceededCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
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
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}

pub struct MarkSubscriptionActivatedCommand {
    pub owner_id: String,
    pub webhook_event: String,
}

impl MarkSubscriptionActivatedCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
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
        .execute(&mut *conn)
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
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
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
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}

impl UpdateBillingAccountFromWebhookCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
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
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}
