use super::*;

fn parse_console_deployment_id() -> Option<i64> {
    std::env::var("CONSOLE_DEPLOYMENT_ID")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
}

fn split_recipients(raw: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    raw.split([',', ';'])
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .filter(|email| seen.insert(email.clone()))
        .collect()
}

pub(super) async fn send_billing_change_email(app_state: &AppState, owner_id: &str, message: &str) {
    let Some(console_deployment_id) = parse_console_deployment_id() else {
        warn!("CONSOLE_DEPLOYMENT_ID not set; skipping billing change email");
        return;
    };

    let account = match GetBillingAccountQuery::new(owner_id.to_string())
        .execute(app_state)
        .await
    {
        Ok(Some(account)) => account,
        Ok(None) => return,
        Err(e) => {
            warn!(
                "Failed to load billing account for {} while sending billing email: {}",
                owner_id, e
            );
            return;
        }
    };

    let recipients = split_recipients(&account.billing_account.billing_email);
    if recipients.is_empty() {
        return;
    }

    let plan_line = account
        .subscription
        .as_ref()
        .and_then(|s| s.plan_name.as_ref())
        .map(|name| format!("Current plan: {}.", name));

    let mut lines = vec![message.to_string()];
    if let Some(plan_line) = plan_line {
        lines.push(plan_line);
    }
    lines.push(
        "You are receiving this email because this email is attached to your Wacht billing account."
            .to_string(),
    );

    let final_message = lines.join("\n");

    let subject = "Billing update".to_string();
    let body_html_lines = lines
        .iter()
        .map(|line| {
            format!(
                "<p style=\"font-size:16px;line-height:1.6;margin:0 0 10px 0;\">{}</p>",
                line
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let body_html = format!("<div>{}</div>", body_html_lines);

    let body_text = final_message.clone();

    for email in recipients {
        if let Err(e) = SendRawEmailCommand::new(
            console_deployment_id,
            email.clone(),
            subject.clone(),
            body_html.clone(),
            Some(body_text.clone()),
        )
        .execute(app_state)
        .await
        {
            warn!(
                "Failed to send billing change email to {} for {}: {}",
                email, owner_id, e
            );
        }
    }
}

pub(super) async fn extract_owner_id(
    app_state: &AppState,
    customer_id: &str,
    data: &serde_json::Value,
) -> String {
    if let Some(metadata) = data["metadata"].as_object() {
        if let Some(owner_id) = metadata.get("owner_id").and_then(|v| v.as_str()) {
            return owner_id.to_string();
        }
    }

    if let Some(customer_metadata) = data["customer"]["metadata"].as_object() {
        if let Some(owner_id) = customer_metadata.get("owner_id").and_then(|v| v.as_str()) {
            return owner_id.to_string();
        }
    }

    if !customer_id.is_empty() {
        if let Ok(Some(owner_id)) = GetBillingAccountByProviderCustomerIdQuery::new(customer_id)
            .execute(app_state)
            .await
        {
            return owner_id;
        }
    }

    String::new()
}
