use super::*;
pub(in crate::project) struct DeploymentSocialConnectionsBulkInsert {
    ids: Vec<i64>,
    deployment_ids: Vec<i64>,
    providers: Vec<String>,
    enableds: Vec<bool>,
    credentials_list: Vec<serde_json::Value>,
    created_ats: Vec<chrono::DateTime<chrono::Utc>>,
    updated_ats: Vec<chrono::DateTime<chrono::Utc>>,
}

impl DeploymentSocialConnectionsBulkInsert {
    pub(in crate::project) fn from_auth_methods<F>(
        deployment_id: i64,
        auth_methods: &[String],
        mut next_id: F,
    ) -> Result<Option<Self>, AppError>
    where
        F: FnMut() -> Result<i64, AppError>,
    {
        let social_providers = [
            "google",
            "apple",
            "facebook",
            "github",
            "microsoft",
            "discord",
            "linkedin",
            "x",
            "gitlab",
        ];

        let mut ids = Vec::new();
        let mut deployment_ids = Vec::new();
        let mut providers = Vec::new();
        let mut enableds = Vec::new();
        let mut credentials_list = Vec::new();
        let mut created_ats = Vec::new();
        let mut updated_ats = Vec::new();

        let now = chrono::Utc::now();

        for provider_name in social_providers {
            let provider_with_oauth = format!("{}_oauth", provider_name);
            let is_selected = auth_methods.iter().any(|method| method == provider_name)
                || auth_methods
                    .iter()
                    .any(|method| method == &provider_with_oauth);
            if !is_selected {
                continue;
            }

            if let Ok(provider) = SocialConnectionProvider::from_str(&provider_with_oauth) {
                ids.push(next_id()?);
                deployment_ids.push(deployment_id);
                providers.push(provider_with_oauth);
                enableds.push(true);
                credentials_list.push(social_credentials_with_default_scopes(&provider)?);
                created_ats.push(now);
                updated_ats.push(now);
            }
        }

        if ids.is_empty() {
            return Ok(None);
        }

        Ok(Some(Self {
            ids,
            deployment_ids,
            providers,
            enableds,
            credentials_list,
            created_ats,
            updated_ats,
        }))
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
                INSERT INTO deployment_social_connections (
                    id,
                    deployment_id,
                    provider,
                    enabled,
                    credentials,
                    created_at,
                    updated_at
                )
                SELECT * FROM UNNEST($1::bigint[], $2::bigint[], $3::text[], $4::bool[], $5::jsonb[], $6::timestamptz[], $7::timestamptz[])
                "#,
            &self.ids,
            &self.deployment_ids,
            &self.providers,
            &self.enableds,
            &self.credentials_list,
            &self.created_ats,
            &self.updated_ats
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}
