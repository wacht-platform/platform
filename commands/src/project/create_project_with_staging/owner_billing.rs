use super::*;

pub(super) fn owner_id_fragment(owner_id: &str) -> Result<&str, AppError> {
    owner_id
        .split('_')
        .next_back()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Validation("Invalid owner id format".to_string()))
}

pub(super) async fn load_billing_account_for_owner(
    owner_id: &str,
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
) -> Result<queries::BillingAccountForOwnerLockResult, AppError> {
    queries::BillingAccountForOwnerLockQuery::builder()
        .owner_id(owner_id)
        .execute_with_db(executor)
        .await?
        .ok_or_else(|| AppError::Validation("No billing account found".to_string()))
}

pub(super) async fn ensure_project_limit_not_reached(
    billing_account_id: i64,
    max_projects_per_account: i64,
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
) -> Result<(), AppError> {
    let max_projects_per_account = if max_projects_per_account > 0 {
        max_projects_per_account
    } else {
        MAX_PROJECTS_PER_BILLING_ACCOUNT
    };

    let project_count = queries::ProjectsCountByBillingAccountQuery::builder()
        .billing_account_id(billing_account_id)
        .execute_with_db(executor)
        .await?;

    if project_count >= max_projects_per_account {
        return Err(AppError::Validation(format!(
            "Project limit reached. You can create up to {} projects.",
            max_projects_per_account
        )));
    }

    Ok(())
}
