use super::*;
pub(in crate::project) struct ProjectInsertedRow {
    pub(in crate::project) id: i64,
    pub(in crate::project) created_at: chrono::DateTime<chrono::Utc>,
    pub(in crate::project) updated_at: chrono::DateTime<chrono::Utc>,
    pub(in crate::project) name: String,
    pub(in crate::project) image_url: String,
    pub(in crate::project) owner_id: Option<String>,
}

#[derive(Default)]
pub(in crate::project) struct ProjectInsert {
    id: Option<i64>,
    name: Option<String>,
    owner_id_fragment: Option<String>,
    billing_account_id: Option<i64>,
}

impl ProjectInsert {
    pub(in crate::project) fn builder() -> Self {
        Self::default()
    }

    pub(in crate::project) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(in crate::project) fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub(in crate::project) fn owner_id_fragment(mut self, owner_id_fragment: impl Into<String>) -> Self {
        self.owner_id_fragment = Some(owner_id_fragment.into());
        self
    }

    pub(in crate::project) fn billing_account_id(mut self, billing_account_id: i64) -> Self {
        self.billing_account_id = Some(billing_account_id);
        self
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<ProjectInsertedRow, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = self
            .id
            .ok_or_else(|| AppError::Validation("project id is required".to_string()))?;
        let name = self
            .name
            .as_deref()
            .ok_or_else(|| AppError::Validation("project name is required".to_string()))?;
        let owner_id_fragment = self.owner_id_fragment.as_deref().ok_or_else(|| {
            AppError::Validation("project owner id fragment is required".to_string())
        })?;
        let billing_account_id = self.billing_account_id.ok_or_else(|| {
            AppError::Validation("project billing_account_id is required".to_string())
        })?;

        let now = chrono::Utc::now();

        let row = sqlx::query!(
            r#"
            INSERT INTO projects (id, name, image_url, owner_id, billing_account_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, created_at, updated_at, name, image_url, owner_id
            "#,
            id,
            name,
            "",
            Some(owner_id_fragment),
            billing_account_id,
            now,
            now,
        )
        .fetch_one(executor)
        .await?;

        Ok(ProjectInsertedRow {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            name: row.name,
            image_url: row.image_url,
            owner_id: row.owner_id,
        })
    }
}
