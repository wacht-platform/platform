use std::collections::HashSet;

use arrow_schema::SchemaRef;
use lancedb::index::Index;
use lancedb::table::OptimizeAction;
use lancedb::{Connection, Error as LanceError, Table};
use models::error::AppError;

const VECTOR_STORE_SUBPATH: &str = "vector";

#[derive(Debug, Clone)]
pub struct VectorStoreConfig {
    pub uri: String,
    pub storage_options: Vec<(String, String)>,
}

pub fn build_vector_store_config(
    bucket: &str,
    root_prefix: Option<&str>,
    endpoint: Option<&str>,
    region: &str,
    access_key_id: Option<&str>,
    secret_access_key: Option<&str>,
    _force_path_style: bool,
) -> VectorStoreConfig {
    let uri = match root_prefix {
        Some(prefix) if !prefix.is_empty() => {
            format!(
                "s3://{}/{}/{}",
                bucket,
                prefix.trim_matches('/'),
                VECTOR_STORE_SUBPATH
            )
        }
        _ => format!("s3://{}/{}", bucket, VECTOR_STORE_SUBPATH),
    };

    let mut storage_options = vec![("region".to_string(), region.to_string())];

    if let Some(endpoint) = endpoint.filter(|value| !value.is_empty()) {
        storage_options.push(("endpoint".to_string(), endpoint.to_string()));
        if endpoint.starts_with("http://") {
            storage_options.push(("allow_http".to_string(), "true".to_string()));
        }
    }

    if let Some(access_key_id) = access_key_id.filter(|value| !value.is_empty()) {
        storage_options.push(("aws_access_key_id".to_string(), access_key_id.to_string()));
    }

    if let Some(secret_access_key) = secret_access_key.filter(|value| !value.is_empty()) {
        storage_options.push((
            "aws_secret_access_key".to_string(),
            secret_access_key.to_string(),
        ));
    }

    VectorStoreConfig {
        uri,
        storage_options,
    }
}

pub fn map_vector_store_error(err: LanceError) -> AppError {
    AppError::Internal(format!("LanceDB error: {}", err))
}

pub async fn connect_vector_store(config: &VectorStoreConfig) -> Result<Connection, AppError> {
    lancedb::connect(&config.uri)
        .storage_options(config.storage_options.clone())
        .execute()
        .await
        .map_err(map_vector_store_error)
}

pub async fn open_table_in_connection(
    conn: &Connection,
    table_name: &str,
) -> Result<Option<Table>, AppError> {
    match conn.open_table(table_name).execute().await {
        Ok(table) => Ok(Some(table)),
        Err(LanceError::TableNotFound { .. }) => Ok(None),
        Err(err) => Err(map_vector_store_error(err)),
    }
}

pub async fn open_or_create_table_in_connection(
    conn: &Connection,
    table_name: &str,
    schema: SchemaRef,
) -> Result<Table, AppError> {
    if let Some(table) = open_table_in_connection(conn, table_name).await? {
        return Ok(table);
    }

    conn.create_empty_table(table_name, schema)
        .execute()
        .await
        .map_err(map_vector_store_error)
}

pub async fn open_table(
    config: &VectorStoreConfig,
    table_name: &str,
) -> Result<Option<Table>, AppError> {
    let conn = connect_vector_store(config).await?;
    open_table_in_connection(&conn, table_name).await
}

pub async fn open_or_create_table(
    config: &VectorStoreConfig,
    table_name: &str,
    schema: SchemaRef,
) -> Result<Table, AppError> {
    let conn = connect_vector_store(config).await?;
    open_or_create_table_in_connection(&conn, table_name, schema).await
}

pub async fn count_rows_with_filter(
    config: &VectorStoreConfig,
    table_name: &str,
    filter: &str,
) -> Result<usize, AppError> {
    let table = match open_table(config, table_name).await? {
        Some(table) => table,
        None => return Ok(0),
    };

    table
        .count_rows(Some(filter.to_string()))
        .await
        .map_err(map_vector_store_error)
}

pub async fn vector_index_exists(
    config: &VectorStoreConfig,
    table_name: &str,
    column: &str,
) -> Result<bool, AppError> {
    let table = match open_table(config, table_name).await? {
        Some(table) => table,
        None => return Ok(false),
    };

    let existing = table.list_indices().await.map_err(map_vector_store_error)?;
    Ok(existing
        .into_iter()
        .any(|idx| idx.columns == vec![column.to_string()]))
}

pub async fn create_auto_vector_index(
    config: &VectorStoreConfig,
    table_name: &str,
    schema: SchemaRef,
    column: &str,
) -> Result<(), AppError> {
    let table = open_or_create_table(config, table_name, schema).await?;
    table
        .create_index(&[column], Index::Auto)
        .execute()
        .await
        .map_err(map_vector_store_error)
}

pub async fn optimize_vector_index(
    config: &VectorStoreConfig,
    table_name: &str,
) -> Result<(), AppError> {
    let table = match open_table(config, table_name).await? {
        Some(table) => table,
        None => return Ok(()),
    };

    table
        .optimize(OptimizeAction::Index(Default::default()))
        .await
        .map_err(map_vector_store_error)?;
    Ok(())
}

pub async fn existing_index_columns(table: &Table) -> Result<HashSet<Vec<String>>, AppError> {
    let existing = table.list_indices().await.map_err(map_vector_store_error)?;
    Ok(existing.into_iter().map(|idx| idx.columns).collect())
}
