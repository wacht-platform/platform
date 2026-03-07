use common::error::AppError;

pub(crate) async fn insert_organization_membership_role<'e, E>(
    executor: E,
    organization_membership_id: i64,
    organization_role_id: i64,
    organization_id: i64,
) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(
        r#"
        INSERT INTO organization_membership_roles (organization_membership_id, organization_role_id, organization_id)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(organization_membership_id)
    .bind(organization_role_id)
    .bind(organization_id)
    .execute(executor)
    .await
    .map_err(AppError::Database)?;

    Ok(())
}
