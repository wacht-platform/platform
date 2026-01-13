use common::error::AppError;
use reqwest::multipart;
use serde_json::Value;

/// ClickUp API client for direct HTTP calls
pub struct ClickUpClient {
    access_token: String,
    client: reqwest::Client,
}

impl ClickUpClient {
    pub fn new(access_token: String) -> Self {
        Self {
            access_token,
            client: reqwest::Client::new(),
        }
    }

    fn auth_header(&self) -> String {
        self.access_token.clone()
    }

    pub async fn get_current_user(&self) -> Result<Value, AppError> {
        let resp = self.client
            .get("https://api.clickup.com/api/v2/user")
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AppError::External(format!("ClickUp API error: {}", e)))?;

        self.handle_response(resp).await
    }

    pub async fn get_teams(&self) -> Result<Value, AppError> {
        let resp = self.client
            .get("https://api.clickup.com/api/v2/team")
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AppError::External(format!("ClickUp API error: {}", e)))?;

        self.handle_response(resp).await
    }

    pub async fn get_spaces(&self, team_id: &str) -> Result<Value, AppError> {
        let resp = self.client
            .get(format!("https://api.clickup.com/api/v2/team/{}/space", team_id))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AppError::External(format!("ClickUp API error: {}", e)))?;

        self.handle_response(resp).await
    }

    pub async fn get_space_lists(&self, space_id: &str) -> Result<Value, AppError> {
        let resp = self.client
            .get(format!("https://api.clickup.com/api/v2/space/{}/list", space_id))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AppError::External(format!("ClickUp API error: {}", e)))?;

        self.handle_response(resp).await
    }

    pub async fn get_task(&self, task_id: &str) -> Result<Value, AppError> {
        let resp = self.client
            .get(format!("https://api.clickup.com/api/v2/task/{}", task_id))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AppError::External(format!("ClickUp API error: {}", e)))?;

        self.handle_response(resp).await
    }

    pub async fn get_tasks(&self, list_id: &str, params: &Value) -> Result<Value, AppError> {
        let mut url = format!("https://api.clickup.com/api/v2/list/{}/task", list_id);
        
        let mut query_params = vec![];
        if let Some(archived) = params.get("archived").and_then(|v| v.as_bool()) {
            query_params.push(format!("archived={}", archived));
        }
        if let Some(page) = params.get("page").and_then(|v| v.as_i64()) {
            query_params.push(format!("page={}", page));
        }
        if let Some(order_by) = params.get("order_by").and_then(|v| v.as_str()) {
            query_params.push(format!("order_by={}", order_by));
        }
        if let Some(reverse) = params.get("reverse").and_then(|v| v.as_bool()) {
            query_params.push(format!("reverse={}", reverse));
        }
        if let Some(subtasks) = params.get("subtasks").and_then(|v| v.as_bool()) {
            query_params.push(format!("subtasks={}", subtasks));
        }
        
        if !query_params.is_empty() {
            url = format!("{}?{}", url, query_params.join("&"));
        }

        let resp = self.client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AppError::External(format!("ClickUp API error: {}", e)))?;

        self.handle_response(resp).await
    }

    pub async fn search_tasks(&self, team_id: &str, params: &Value) -> Result<Value, AppError> {
        let mut url = format!("https://api.clickup.com/api/v2/team/{}/task", team_id);
        
        let mut query_params = vec![];
        if let Some(search) = params.get("search").and_then(|v| v.as_str()) {
            query_params.push(format!("search={}", urlencoding::encode(search)));
        }
        if let Some(archived) = params.get("archived").and_then(|v| v.as_bool()) {
            query_params.push(format!("archived={}", archived));
        }
        if let Some(page) = params.get("page").and_then(|v| v.as_i64()) {
            query_params.push(format!("page={}", page));
        }
        if let Some(arr) = params.get("assignees").and_then(|v| v.as_array()) {
            for a in arr {
                if let Some(s) = a.as_str() {
                    query_params.push(format!("assignees[]={}", s));
                }
            }
        }
        if let Some(arr) = params.get("statuses").and_then(|v| v.as_array()) {
            for s in arr {
                if let Some(status) = s.as_str() {
                    query_params.push(format!("statuses[]={}", urlencoding::encode(status)));
                }
            }
        }
        
        if !query_params.is_empty() {
            url = format!("{}?{}", url, query_params.join("&"));
        }

        let resp = self.client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| AppError::External(format!("ClickUp API error: {}", e)))?;

        self.handle_response(resp).await
    }

    // ========== Write Operations ==========

    pub async fn create_task(&self, list_id: &str, params: &Value) -> Result<Value, AppError> {
        let resp = self.client
            .post(format!("https://api.clickup.com/api/v2/list/{}/task", list_id))
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .json(params)
            .send()
            .await
            .map_err(|e| AppError::External(format!("ClickUp API error: {}", e)))?;

        self.handle_response(resp).await
    }

    pub async fn update_task(&self, task_id: &str, params: &Value) -> Result<Value, AppError> {
        let resp = self.client
            .put(format!("https://api.clickup.com/api/v2/task/{}", task_id))
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .json(params)
            .send()
            .await
            .map_err(|e| AppError::External(format!("ClickUp API error: {}", e)))?;

        self.handle_response(resp).await
    }

    pub async fn add_comment(&self, task_id: &str, params: &Value) -> Result<Value, AppError> {
        let resp = self.client
            .post(format!("https://api.clickup.com/api/v2/task/{}/comment", task_id))
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .json(params)
            .send()
            .await
            .map_err(|e| AppError::External(format!("ClickUp API error: {}", e)))?;

        self.handle_response(resp).await
    }

    pub async fn create_list(&self, space_id: &str, params: &Value) -> Result<Value, AppError> {
        let resp = self.client
            .post(format!("https://api.clickup.com/api/v2/space/{}/list", space_id))
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .json(params)
            .send()
            .await
            .map_err(|e| AppError::External(format!("ClickUp API error: {}", e)))?;

        self.handle_response(resp).await
    }

    pub async fn add_attachment(
        &self,
        task_id: &str,
        filename: &str,
        mime_type: &str,
        file_data: Vec<u8>,
    ) -> Result<Value, AppError> {
        let part = multipart::Part::bytes(file_data)
            .file_name(filename.to_string())
            .mime_str(mime_type)
            .map_err(|e| AppError::External(format!("Invalid mime type: {}", e)))?;

        let form = multipart::Form::new().part("attachment", part);

        let resp = self.client
            .post(format!("https://api.clickup.com/api/v2/task/{}/attachment", task_id))
            .header("Authorization", self.auth_header())
            .multipart(form)
            .send()
            .await
            .map_err(|e| AppError::External(format!("ClickUp attachment upload error: {}", e)))?;

        self.handle_response(resp).await
    }

    // ========== Helper ==========

    async fn handle_response(&self, resp: reqwest::Response) -> Result<Value, AppError> {
        let status = resp.status();
        let body = resp.text().await
            .map_err(|e| AppError::External(format!("Failed to read response: {}", e)))?;

        if !status.is_success() {
            return Err(AppError::External(format!("ClickUp API error ({}): {}", status, body)));
        }

        serde_json::from_str(&body)
            .map_err(|e| AppError::External(format!("Failed to parse ClickUp response: {}", e)))
    }
}
