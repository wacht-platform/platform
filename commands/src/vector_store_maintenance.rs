use chrono::{Duration, Utc};
use common::{
    HasDbRouter, HasEncryptionProvider, HasNatsProvider, count_indexable_knowledge_base_rows,
    count_indexable_memory_rows_for_dimension, create_knowledge_base_vector_index,
    create_memory_vector_index_for_dimension, error::AppError, knowledge_base_vector_index_exists,
    optimize_knowledge_base_vector_index, optimize_memory_vector_index,
};
use dto::json::nats::NatsTaskMessage;
use models::{default_embedding_dimension, is_supported_embedding_dimension};
use queries::GetDeploymentAiSettingsQuery;

use crate::ResolveDeploymentStorageCommand;

pub const VECTOR_STORE_KNOWLEDGE_BASE: &str = "knowledge_base";
pub const VECTOR_STORE_MEMORY: &str = "memory";

const VECTOR_INDEX_CREATE_THRESHOLD_ROWS: i64 = 3_000;
const VECTOR_INDEX_OPTIMIZE_INTERVAL_HOURS: i64 = 4;

pub struct DispatchVectorStoreMaintenanceTaskCommand {
    pub deployment_id: i64,
    pub store_name: String,
    pub source_key: String,
}

impl DispatchVectorStoreMaintenanceTaskCommand {
    pub fn new(deployment_id: i64, store_name: String, source_key: String) -> Self {
        Self {
            deployment_id,
            store_name,
            source_key,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasNatsProvider + ?Sized,
    {
        let task_message = NatsTaskMessage {
            task_type: "vector_store.maintain".to_string(),
            task_id: format!(
                "vector-store-maintain-{}-{}-{}",
                self.deployment_id, self.store_name, self.source_key
            ),
            payload: serde_json::json!({
                "deployment_id": self.deployment_id,
                "store_name": self.store_name
            }),
        };

        deps.nats_provider()
            .publish(
                "worker.tasks.vector_store.maintain",
                serde_json::to_vec(&task_message)
                    .map_err(|e| AppError::Internal(format!("Failed to serialize task: {}", e)))?
                    .into(),
            )
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "Failed to publish vector store maintenance task to NATS: {}",
                    e
                ))
            })?;

        Ok(())
    }
}

pub struct MaintainVectorStoreIndexCommand {
    pub deployment_id: i64,
    pub store_name: String,
}

impl MaintainVectorStoreIndexCommand {
    pub fn new(deployment_id: i64, store_name: String) -> Self {
        Self {
            deployment_id,
            store_name,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<String, AppError>
    where
        D: HasDbRouter + HasEncryptionProvider + ?Sized,
    {
        let now = Utc::now();
        let deployment_id = self.deployment_id;
        let store_name = self.store_name.clone();

        let storage = ResolveDeploymentStorageCommand::new(deployment_id)
            .execute_with_deps(deps)
            .await?;
        let lance_config = storage.vector_store_config();
        let embedding_dimension = GetDeploymentAiSettingsQuery::new(deployment_id)
            .execute_with_db(deps.writer_pool())
            .await?
            .map(|settings| settings.embedding_dimension)
            .unwrap_or_else(default_embedding_dimension);
        if !is_supported_embedding_dimension(embedding_dimension) {
            return Err(AppError::Validation(format!(
                "Unsupported embedding dimension {} for deployment {}",
                embedding_dimension, deployment_id
            )));
        }

        let (tracked_row_count, vector_index_exists) = match store_name.as_str() {
            VECTOR_STORE_KNOWLEDGE_BASE => (
                count_indexable_knowledge_base_rows(&lance_config, embedding_dimension).await?
                    as i64,
                knowledge_base_vector_index_exists(&lance_config, embedding_dimension).await?,
            ),
            VECTOR_STORE_MEMORY => (
                count_indexable_memory_rows_for_dimension(&lance_config, embedding_dimension)
                    .await? as i64,
                common::memory_vector_index_exists_for_dimension(
                    &lance_config,
                    embedding_dimension,
                )
                .await?,
            ),
            other => {
                return Err(AppError::Validation(format!(
                    "Unsupported vector store maintenance target: {}",
                    other
                )));
            }
        };

        let state = sqlx::query!(
            r#"
            SELECT
                tracked_row_count,
                vector_index_created_at,
                last_index_attempt_at,
                last_optimized_at
            FROM deployment_vector_index_states
            WHERE deployment_id = $1 AND store_name = $2
            "#,
            deployment_id,
            &store_name
        )
        .fetch_optional(deps.writer_pool())
        .await
        .map_err(AppError::Database)?;

        if state.is_none() {
            sqlx::query!(
                r#"
                INSERT INTO deployment_vector_index_states (
                    deployment_id,
                    store_name,
                    tracked_row_count,
                    vector_index_created_at,
                    last_index_attempt_at,
                    last_optimized_at,
                    created_at,
                    updated_at
                ) VALUES ($1, $2, $3, $4, NULL, NULL, $5, $5)
                "#,
                deployment_id,
                &store_name,
                tracked_row_count,
                if vector_index_exists { Some(now) } else { None },
                now
            )
            .execute(deps.writer_pool())
            .await
            .map_err(AppError::Database)?;
        } else {
            sqlx::query!(
                r#"
                UPDATE deployment_vector_index_states
                SET
                    tracked_row_count = $3,
                    vector_index_created_at = CASE
                        WHEN vector_index_created_at IS NULL AND $4::boolean THEN $5
                        ELSE vector_index_created_at
                    END,
                    updated_at = $5
                WHERE deployment_id = $1 AND store_name = $2
                "#,
                deployment_id,
                &store_name,
                tracked_row_count,
                vector_index_exists,
                now
            )
            .execute(deps.writer_pool())
            .await
            .map_err(AppError::Database)?;
        }

        let state = sqlx::query!(
            r#"
            SELECT
                tracked_row_count,
                vector_index_created_at,
                last_index_attempt_at,
                last_optimized_at
            FROM deployment_vector_index_states
            WHERE deployment_id = $1 AND store_name = $2
            "#,
            deployment_id,
            &store_name
        )
        .fetch_one(deps.writer_pool())
        .await
        .map_err(AppError::Database)?;

        if !vector_index_exists {
            if tracked_row_count < VECTOR_INDEX_CREATE_THRESHOLD_ROWS {
                return Ok(format!(
                    "Skipped vector index creation for {}: only {} indexable rows (threshold {})",
                    store_name, tracked_row_count, VECTOR_INDEX_CREATE_THRESHOLD_ROWS
                ));
            }

            sqlx::query!(
                r#"
                UPDATE deployment_vector_index_states
                SET
                    tracked_row_count = $3,
                    last_index_attempt_at = $4,
                    updated_at = $4
                WHERE deployment_id = $1 AND store_name = $2
                "#,
                deployment_id,
                &store_name,
                tracked_row_count,
                now
            )
            .execute(deps.writer_pool())
            .await
            .map_err(AppError::Database)?;

            let create_result = match store_name.as_str() {
                VECTOR_STORE_KNOWLEDGE_BASE => {
                    create_knowledge_base_vector_index(&lance_config, embedding_dimension).await
                }
                VECTOR_STORE_MEMORY => {
                    create_memory_vector_index_for_dimension(&lance_config, embedding_dimension)
                        .await
                }
                _ => unreachable!(),
            };

            match create_result {
                Ok(()) => {
                    sqlx::query!(
                        r#"
                        UPDATE deployment_vector_index_states
                        SET
                            tracked_row_count = $3,
                            vector_index_created_at = $4,
                            last_index_attempt_at = $4,
                            updated_at = $4
                        WHERE deployment_id = $1 AND store_name = $2
                        "#,
                        deployment_id,
                        &store_name,
                        tracked_row_count,
                        now
                    )
                    .execute(deps.writer_pool())
                    .await
                    .map_err(AppError::Database)?;

                    return Ok(format!(
                        "Created vector index for {} at {} indexable rows",
                        store_name, tracked_row_count
                    ));
                }
                Err(error) => {
                    tracing::warn!(
                        deployment_id,
                        store_name = %store_name,
                        tracked_row_count,
                        error = %error,
                        "Vector index creation attempt failed"
                    );
                    return Ok(format!(
                        "Vector index creation attempt failed for {} after reaching {} rows: {}",
                        store_name, tracked_row_count, error
                    ));
                }
            }
        }

        let should_optimize = state.last_optimized_at.map_or(true, |last| {
            now.signed_duration_since(last) >= Duration::hours(VECTOR_INDEX_OPTIMIZE_INTERVAL_HOURS)
        });

        if !should_optimize {
            return Ok(format!(
                "Skipped optimize for {}: last optimize was too recent",
                store_name
            ));
        }

        let optimize_result = match store_name.as_str() {
            VECTOR_STORE_KNOWLEDGE_BASE => {
                optimize_knowledge_base_vector_index(&lance_config).await
            }
            VECTOR_STORE_MEMORY => optimize_memory_vector_index(&lance_config).await,
            _ => unreachable!(),
        };

        match optimize_result {
            Ok(()) => {
                sqlx::query!(
                    r#"
                    UPDATE deployment_vector_index_states
                    SET
                        tracked_row_count = $3,
                        last_optimized_at = $4,
                        updated_at = $4
                    WHERE deployment_id = $1 AND store_name = $2
                    "#,
                    deployment_id,
                    &store_name,
                    tracked_row_count,
                    now
                )
                .execute(deps.writer_pool())
                .await
                .map_err(AppError::Database)?;

                Ok(format!(
                    "Optimized vector index for {} at {} indexable rows",
                    store_name, tracked_row_count
                ))
            }
            Err(error) => {
                tracing::warn!(
                    deployment_id,
                    store_name = %store_name,
                    tracked_row_count,
                    error = %error,
                    "Vector index optimize attempt failed"
                );
                Ok(format!(
                    "Vector index optimize attempt failed for {}: {}",
                    store_name, error
                ))
            }
        }
    }
}
