use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::pulse_transaction::{PulseTransaction, PulseTransactionType};
use tracing::warn;

pub struct AddPulseCreditsCommand {
    pub owner_id: String,
    pub amount_pulse_cents: i64,
    pub transaction_type: PulseTransactionType,
    pub reference_id: Option<String>,
}

impl Command for AddPulseCreditsCommand {
    type Output = PulseTransaction;

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        let mut tx = state.db_pool.begin().await?;

        let row = sqlx::query("SELECT id FROM billing_accounts WHERE owner_id = $1")
            .bind(&self.owner_id)
            .fetch_one(&mut *tx)
            .await?;

        use sqlx::Row;
        let account_id: i64 = row.get("id");

        sqlx::query("UPDATE billing_accounts SET pulse_balance_cents = pulse_balance_cents + $1, updated_at = NOW() WHERE id = $2")
            .bind(self.amount_pulse_cents)
            .bind(account_id)
            .execute(&mut *tx)
            .await?;

        // 3. Log transaction
        let transaction_id = state.sf.next_id().unwrap() as i64;
        let transaction = sqlx::query_as::<_, PulseTransaction>(
            r#"
            INSERT INTO pulse_transactions (
                id,
                billing_account_id,
                amount_pulse_cents,
                transaction_type,
                reference_id,
                created_at
            ) VALUES ($1, $2, $3, $4, $5, NOW())
            RETURNING id, billing_account_id, amount_pulse_cents, transaction_type, reference_id, created_at
            "#
        )
        .bind(transaction_id)
        .bind(account_id)
        .bind(self.amount_pulse_cents)
        .bind(&self.transaction_type)
        .bind(&self.reference_id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(transaction)
    }
}

pub struct DeductPulseCreditsCommand {
    pub owner_id: String,
    pub amount_pulse_cents: i64,
    pub transaction_type: PulseTransactionType,
    pub reference_id: Option<String>,
}

impl Command for DeductPulseCreditsCommand {
    type Output = PulseTransaction;

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        let mut tx = state.db_pool.begin().await?;

        let row = sqlx::query(
            "SELECT id, pulse_balance_cents FROM billing_accounts WHERE owner_id = $1 FOR UPDATE",
        )
        .bind(&self.owner_id)
        .fetch_one(&mut *tx)
        .await?;

        use sqlx::Row;
        let account_id: i64 = row.get("id");
        let pulse_balance_cents: i64 = row.get("pulse_balance_cents");

        if pulse_balance_cents < self.amount_pulse_cents {
            warn!("Insufficient Pulse Credits");
        }

        sqlx::query("UPDATE billing_accounts SET pulse_balance_cents = pulse_balance_cents - $1, updated_at = NOW() WHERE id = $2")
            .bind(self.amount_pulse_cents)
            .bind(account_id)
            .execute(&mut *tx)
            .await?;

        let transaction_id = state.sf.next_id().unwrap() as i64;
        let transaction = sqlx::query_as::<_, PulseTransaction>(
            r#"
            INSERT INTO pulse_transactions (
                id,
                billing_account_id,
                amount_pulse_cents,
                transaction_type,
                reference_id,
                created_at
            ) VALUES ($1, $2, $3, $4, $5, NOW())
            RETURNING id, billing_account_id, amount_pulse_cents, transaction_type, reference_id, created_at
            "#
        )
        .bind(transaction_id)
        .bind(account_id)
        .bind(-self.amount_pulse_cents)
        .bind(&self.transaction_type)
        .bind(&self.reference_id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(transaction)
    }
}
