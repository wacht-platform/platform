use super::*;

pub(super) async fn load_project_with_billing_for_staging(
    project_id: i64,
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
) -> Result<queries::ProjectWithBillingForStagingRow, AppError> {
    queries::ProjectWithBillingForStagingQuery::builder()
        .project_id(project_id)
        .execute_with_db(executor)
        .await?
        .ok_or_else(|| project_not_found(project_id))
}
