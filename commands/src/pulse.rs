use crate::notification::CreateNotificationCommand;
use common::{HasDbRouter, HasIdProvider, HasNatsProvider, error::AppError};
use models::notification::NotificationSeverity;
use models::pulse_transaction::{PulseTransaction, PulseTransactionType};
use tracing::warn;

const LOW_BALANCE_THRESHOLD_CENTS: i64 = 500;
const DISABLE_THRESHOLD_CENTS: i64 = -500;
const AMOUNT_MUST_BE_POSITIVE: &str = "amount_pulse_cents must be greater than zero";

fn require_positive_amount(amount_pulse_cents: i64) -> Result<(), AppError> {
    if amount_pulse_cents <= 0 {
        return Err(AppError::BadRequest(AMOUNT_MUST_BE_POSITIVE.to_string()));
    }
    Ok(())
}

fn require_transaction_id(transaction_id: Option<i64>) -> Result<i64, AppError> {
    transaction_id.ok_or_else(|| AppError::Validation("transaction_id is required".to_string()))
}

struct BillingPulseState {
    account_id: i64,
    pulse_balance_cents: i64,
    pulse_usage_disabled: bool,
    notified_below_five: bool,
    notified_below_zero: bool,
    notified_disabled: bool,
}

struct PulseTransition {
    usage_disabled_changed_to: Option<bool>,
    notify_below_five: bool,
    notify_below_zero: bool,
    notify_disabled: bool,
}

fn compute_transition(old_balance: i64, new_balance: i64, old_disabled: bool) -> PulseTransition {
    let new_disabled = if old_disabled {
        new_balance <= LOW_BALANCE_THRESHOLD_CENTS
    } else {
        new_balance <= DISABLE_THRESHOLD_CENTS
    };

    PulseTransition {
        usage_disabled_changed_to: (old_disabled != new_disabled).then_some(new_disabled),
        notify_below_five: old_balance >= LOW_BALANCE_THRESHOLD_CENTS
            && new_balance < LOW_BALANCE_THRESHOLD_CENTS,
        notify_below_zero: old_balance >= 0 && new_balance < 0,
        notify_disabled: !old_disabled && new_disabled,
    }
}

fn parse_user_id_from_owner_id(owner_id: &str) -> Option<i64> {
    owner_id.strip_prefix("user_")?.parse::<i64>().ok()
}

async fn find_notification_deployment_id(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    billing_account_id: i64,
) -> Result<Option<i64>, AppError> {
    let row = sqlx::query!(
        r#"
        SELECT d.id
        FROM deployments d
        JOIN projects p ON p.id = d.project_id
        WHERE p.billing_account_id = $1
          AND d.deleted_at IS NULL
        ORDER BY d.created_at ASC
        LIMIT 1
        "#,
        billing_account_id
    )
    .fetch_optional(&mut **tx)
    .await?;

    Ok(row.map(|r| r.id))
}

async fn create_pulse_threshold_notifications<D>(
    deps: &D,
    owner_id: &str,
    deployment_id: i64,
    transition: &PulseTransition,
) -> Result<(), AppError>
where
    D: HasDbRouter + HasNatsProvider,
{
    let Some(user_id) = parse_user_id_from_owner_id(owner_id) else {
        return Ok(());
    };

    if transition.notify_below_five {
        CreateNotificationCommand::new(
            deployment_id,
            "Pulse credits running low".to_string(),
            "Your Pulse balance is below $5. Add credits to avoid interruptions.".to_string(),
        )
        .with_user(user_id)
        .with_severity(NotificationSeverity::Warning)
        .execute_with_deps(deps)
        .await?;
    }

    if transition.notify_below_zero {
        CreateNotificationCommand::new(
            deployment_id,
            "Pulse balance is negative".to_string(),
            "Your Pulse balance is below $0. Add credits soon to keep AI and SMS active."
                .to_string(),
        )
        .with_user(user_id)
        .with_severity(NotificationSeverity::Warning)
        .execute_with_deps(deps)
        .await?;
    }

    if transition.notify_disabled {
        CreateNotificationCommand::new(
            deployment_id,
            "AI and SMS paused".to_string(),
            "Your Pulse balance reached -$5. AI and SMS are paused until your balance goes above $5."
                .to_string(),
        )
        .with_user(user_id)
        .with_severity(NotificationSeverity::Error)
        .execute_with_deps(deps)
        .await?;
    }

    Ok(())
}

pub struct AddPulseCreditsCommand {
    pub transaction_id: Option<i64>,
    pub owner_id: String,
    pub amount_pulse_cents: i64,
    pub transaction_type: PulseTransactionType,
    pub reference_id: Option<String>,
}

impl AddPulseCreditsCommand {
    pub fn new(
        owner_id: String,
        amount_pulse_cents: i64,
        transaction_type: PulseTransactionType,
    ) -> Self {
        Self {
            transaction_id: None,
            owner_id,
            amount_pulse_cents,
            transaction_type,
            reference_id: None,
        }
    }

    pub fn with_reference_id(mut self, reference_id: Option<String>) -> Self {
        self.reference_id = reference_id;
        self
    }

    pub fn with_transaction_id(mut self, transaction_id: i64) -> Self {
        self.transaction_id = Some(transaction_id);
        self
    }

    pub async fn execute_with_db<'a, Db>(self, db: Db) -> Result<PulseTransaction, AppError>
    where
        Db: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        require_positive_amount(self.amount_pulse_cents)?;
        let transaction_id = require_transaction_id(self.transaction_id)?;

        let mut tx = db.begin().await?;

        let row = sqlx::query!(
            r#"
            SELECT
                id,
                pulse_balance_cents,
                COALESCE(pulse_usage_disabled, false) AS "pulse_usage_disabled!"
            FROM billing_accounts
            WHERE owner_id = $1
            FOR UPDATE
            "#,
            self.owner_id
        )
        .fetch_one(&mut *tx)
        .await?;

        let current_state = BillingPulseState {
            account_id: row.id,
            pulse_balance_cents: row.pulse_balance_cents,
            pulse_usage_disabled: row.pulse_usage_disabled,
            notified_below_five: false,
            notified_below_zero: false,
            notified_disabled: false,
        };
        let old_balance = current_state.pulse_balance_cents;
        let new_balance = old_balance + self.amount_pulse_cents;
        let transition =
            compute_transition(old_balance, new_balance, current_state.pulse_usage_disabled);
        let new_disabled = transition
            .usage_disabled_changed_to
            .unwrap_or(current_state.pulse_usage_disabled);

        sqlx::query!(
            r#"
            UPDATE billing_accounts
            SET
                pulse_balance_cents = $1,
                pulse_usage_disabled = $2,
                updated_at = NOW()
            WHERE id = $3
            "#,
            new_balance,
            new_disabled,
            current_state.account_id
        )
        .execute(&mut *tx)
        .await?;

        // 3. Log transaction
        let transaction = sqlx::query_as!(
            PulseTransaction,
            r#"
            INSERT INTO pulse_transactions (
                id,
                billing_account_id,
                amount_pulse_cents,
                transaction_type,
                reference_id,
                created_at
            ) VALUES ($1, $2, $3, $4, $5, NOW())
            RETURNING id, billing_account_id, amount_pulse_cents,
                      transaction_type as "transaction_type: PulseTransactionType",
                      reference_id, created_at
            "#,
            transaction_id,
            current_state.account_id,
            self.amount_pulse_cents,
            self.transaction_type as _,
            self.reference_id
        )
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(transaction)
    }
}

pub struct DeductPulseCreditsCommand {
    pub transaction_id: Option<i64>,
    pub owner_id: String,
    pub amount_pulse_cents: i64,
    pub transaction_type: PulseTransactionType,
    pub reference_id: Option<String>,
}

impl DeductPulseCreditsCommand {
    pub fn with_transaction_id(mut self, transaction_id: i64) -> Self {
        self.transaction_id = Some(transaction_id);
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<PulseTransaction, AppError>
    where
        D: HasDbRouter + HasNatsProvider + HasIdProvider,
    {
        let transaction_id = self
            .transaction_id
            .unwrap_or(deps.id_provider().next_id()? as i64);
        require_positive_amount(self.amount_pulse_cents)?;

        let mut tx = deps.writer_pool().begin().await?;

        let row = sqlx::query!(
            r#"
            SELECT
                id,
                pulse_balance_cents,
                COALESCE(pulse_usage_disabled, false) AS "pulse_usage_disabled!",
                COALESCE(pulse_notified_below_five, false) AS "pulse_notified_below_five!",
                COALESCE(pulse_notified_below_zero, false) AS "pulse_notified_below_zero!",
                COALESCE(pulse_notified_disabled, false) AS "pulse_notified_disabled!"
            FROM billing_accounts
            WHERE owner_id = $1
            FOR UPDATE
            "#,
            self.owner_id
        )
        .fetch_one(&mut *tx)
        .await?;

        let current_state = BillingPulseState {
            account_id: row.id,
            pulse_balance_cents: row.pulse_balance_cents,
            pulse_usage_disabled: row.pulse_usage_disabled,
            notified_below_five: row.pulse_notified_below_five,
            notified_below_zero: row.pulse_notified_below_zero,
            notified_disabled: row.pulse_notified_disabled,
        };

        if current_state.pulse_balance_cents < self.amount_pulse_cents {
            warn!("Insufficient Pulse Credits");
        }

        let old_balance = current_state.pulse_balance_cents;
        let requested_new_balance = old_balance - self.amount_pulse_cents;
        let new_balance = requested_new_balance.max(DISABLE_THRESHOLD_CENTS);
        let deducted_amount = old_balance - new_balance;
        let transition =
            compute_transition(old_balance, new_balance, current_state.pulse_usage_disabled);
        let new_disabled = transition
            .usage_disabled_changed_to
            .unwrap_or(current_state.pulse_usage_disabled);
        let should_mark_below_five =
            transition.notify_below_five && !current_state.notified_below_five;
        let should_mark_below_zero =
            transition.notify_below_zero && !current_state.notified_below_zero;
        let should_mark_disabled = transition.notify_disabled && !current_state.notified_disabled;

        sqlx::query!(
            r#"
            UPDATE billing_accounts
            SET
                pulse_balance_cents = $1,
                pulse_usage_disabled = $2,
                pulse_notified_below_five = pulse_notified_below_five OR $3,
                pulse_notified_below_zero = pulse_notified_below_zero OR $4,
                pulse_notified_disabled = pulse_notified_disabled OR $5,
                updated_at = NOW()
            WHERE id = $6
            "#,
            new_balance,
            new_disabled,
            should_mark_below_five,
            should_mark_below_zero,
            should_mark_disabled,
            current_state.account_id
        )
        .execute(&mut *tx)
        .await?;

        let notification_deployment_id =
            if should_mark_below_five || should_mark_below_zero || should_mark_disabled {
                find_notification_deployment_id(&mut tx, current_state.account_id).await?
            } else {
                None
            };

        let transaction = sqlx::query_as!(
            PulseTransaction,
            r#"
            INSERT INTO pulse_transactions (
                id,
                billing_account_id,
                amount_pulse_cents,
                transaction_type,
                reference_id,
                created_at
            ) VALUES ($1, $2, $3, $4, $5, NOW())
            RETURNING id, billing_account_id, amount_pulse_cents,
                      transaction_type as "transaction_type: PulseTransactionType",
                      reference_id, created_at
            "#,
            transaction_id,
            current_state.account_id,
            -deducted_amount,
            self.transaction_type as _,
            self.reference_id
        )
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        if let Some(deployment_id) = notification_deployment_id {
            let mut notify_transition = transition;
            notify_transition.notify_below_five = should_mark_below_five;
            notify_transition.notify_below_zero = should_mark_below_zero;
            notify_transition.notify_disabled = should_mark_disabled;
            create_pulse_threshold_notifications(
                deps,
                &self.owner_id,
                deployment_id,
                &notify_transition,
            )
            .await?;
        }

        Ok(transaction)
    }
}

pub struct EnsurePulseUsageAllowedForDeploymentCommand {
    pub deployment_id: i64,
}

impl EnsurePulseUsageAllowedForDeploymentCommand {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl EnsurePulseUsageAllowedForDeploymentCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT COALESCE(ba.pulse_usage_disabled, false) AS "pulse_usage_disabled!"
            FROM deployments d
            JOIN projects p ON p.id = d.project_id
            JOIN billing_accounts ba ON ba.id = p.billing_account_id
            WHERE d.id = $1
            "#,
            self.deployment_id
        )
        .fetch_one(executor)
        .await?;

        let disabled = row.pulse_usage_disabled;
        if disabled {
            return Err(AppError::Forbidden("Pulse usage is paused".to_string()));
        }

        Ok(())
    }
}
