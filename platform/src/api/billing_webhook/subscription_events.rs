use super::notifications::{extract_owner_id, send_billing_change_email};
use axum::http::StatusCode;
use commands::{
    Command,
    billing::{
        MarkCheckoutFlowFailedCommand, MarkSubscriptionActivatedCommand,
        UpdateBillingAccountStatusCommand, UpsertSubscriptionCommand,
    },
};
use common::state::AppState;
use tracing::{error, info, warn};

use super::get_customer_id;

fn next_subscription_id(app_state: &AppState) -> Result<i64, StatusCode> {
    app_state.sf.next_id().map(|id| id as i64).map_err(|e| {
        error!("Failed to generate subscription id: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

pub(super) async fn handle_subscription_active(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let product_id = data["product_id"].as_str().map(|s| s.to_string());
    let status = data["status"].as_str().unwrap_or("active");
    let previous_billing_date = data["previous_billing_date"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    if customer_id.is_empty() || subscription_id.is_empty() {
        warn!("Missing customer_id or subscription_id in subscription webhook");
        return Ok(());
    }

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if owner_id.is_empty() {
        warn!(
            "Could not determine owner_id from customer_id: {}",
            customer_id
        );
        return Ok(());
    }

    UpsertSubscriptionCommand::new(
            next_subscription_id(app_state)?,
            owner_id.clone(),
            customer_id.to_string(),
            subscription_id.to_string(),
            status.to_string(),
        )
        .with_product_id(product_id)
        .with_previous_billing_date(previous_billing_date)
        .execute_with_db(app_state.db_router.writer())
    .await
    .map_err(|e| {
        error!("Failed to upsert subscription: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    UpdateBillingAccountStatusCommand::new(owner_id.clone(), status.to_string())
    .execute_with_db(app_state.db_router.writer())
    .await
    .map_err(|e| {
        error!("Failed to update billing account status: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    MarkSubscriptionActivatedCommand::new(owner_id.clone(), "subscription.active".to_string())
        .execute_with_db(app_state.db_router.writer())
    .await
    .map_err(|e| {
        error!("Failed to update checkout flow state: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    info!(
        "Subscription {} activated for owner {}",
        subscription_id, owner_id
    );
    send_billing_change_email(app_state, &owner_id, "Your subscription is now active.").await;

    Ok(())
}

pub(super) async fn handle_subscription_renewed(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let product_id = data["product_id"].as_str().map(|s| s.to_string());
    let previous_billing_date = data["previous_billing_date"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if !owner_id.is_empty() {
        UpsertSubscriptionCommand::new(
            next_subscription_id(app_state)?,
            owner_id.clone(),
            customer_id.to_string(),
            subscription_id.to_string(),
            "active".to_string(),
        )
        .with_product_id(product_id)
        .with_previous_billing_date(previous_billing_date)
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update subscription on renewal: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        MarkSubscriptionActivatedCommand::new(owner_id.clone(), "subscription.renewed".to_string())
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update checkout flow state: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!(
            "Subscription {} renewed for owner {}",
            subscription_id, owner_id
        );
        send_billing_change_email(
            app_state,
            &owner_id,
            "Your subscription was renewed successfully.",
        )
        .await;
    }

    Ok(())
}

pub(super) async fn handle_subscription_plan_changed(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let new_product_id = data["product_id"].as_str().unwrap_or("");

    let previous_billing_date = data["previous_billing_date"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if !owner_id.is_empty() {
        UpsertSubscriptionCommand::new(
            next_subscription_id(app_state)?,
            owner_id.clone(),
            customer_id.to_string(),
            subscription_id.to_string(),
            "active".to_string(),
        )
        .with_product_id(Some(new_product_id.to_string()))
        .with_previous_billing_date(previous_billing_date)
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update subscription on plan change: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        MarkSubscriptionActivatedCommand::new(owner_id.clone(), "subscription.plan_changed".to_string())
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update checkout flow state: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!(
            "Plan changed for subscription {} to product {} (owner: {})",
            subscription_id, new_product_id, owner_id
        );
        send_billing_change_email(app_state, &owner_id, "Your subscription plan was updated.")
            .await;
    }

    Ok(())
}

pub(super) async fn handle_subscription_cancelled(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let product_id = data["product_id"].as_str().map(|s| s.to_string());
    let previous_billing_date = data["previous_billing_date"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let status = data["status"].as_str().unwrap_or("cancelled");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if !owner_id.is_empty() {
        UpsertSubscriptionCommand::new(
            next_subscription_id(app_state)?,
            owner_id.clone(),
            customer_id.to_string(),
            subscription_id.to_string(),
            status.to_string(),
        )
        .with_product_id(product_id)
        .with_previous_billing_date(previous_billing_date)
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update subscription status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        UpdateBillingAccountStatusCommand::new(owner_id.clone(), status.to_string())
    .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update billing account status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        MarkCheckoutFlowFailedCommand::new(owner_id.clone(), "subscription.cancelled".to_string(), status.to_string())
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update checkout flow state: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!(
            "Subscription {} cancelled for owner {}",
            subscription_id, owner_id
        );
        send_billing_change_email(app_state, &owner_id, "Your subscription was cancelled.").await;
    }

    Ok(())
}

pub(super) async fn handle_subscription_on_hold(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let product_id = data["product_id"].as_str().map(|s| s.to_string());
    let previous_billing_date = data["previous_billing_date"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let status = data["status"].as_str().unwrap_or("on_hold");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if !owner_id.is_empty() {
        UpsertSubscriptionCommand::new(
            next_subscription_id(app_state)?,
            owner_id.clone(),
            customer_id.to_string(),
            subscription_id.to_string(),
            status.to_string(),
        )
        .with_product_id(product_id)
        .with_previous_billing_date(previous_billing_date)
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update subscription status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        UpdateBillingAccountStatusCommand::new(owner_id.clone(), status.to_string())
    .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update billing account status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        MarkCheckoutFlowFailedCommand::new(owner_id.clone(), "subscription.on_hold".to_string(), status.to_string())
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update checkout flow state: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!(
            "Subscription {} on hold for owner {}",
            subscription_id, owner_id
        );
        send_billing_change_email(
            app_state,
            &owner_id,
            "Your subscription is currently on hold.",
        )
        .await;
    }

    Ok(())
}

pub(super) async fn handle_subscription_failed(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let product_id = data["product_id"].as_str().map(|s| s.to_string());
    let previous_billing_date = data["previous_billing_date"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let status = data["status"].as_str().unwrap_or("failed");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if !owner_id.is_empty() {
        UpsertSubscriptionCommand::new(
            next_subscription_id(app_state)?,
            owner_id.clone(),
            customer_id.to_string(),
            subscription_id.to_string(),
            status.to_string(),
        )
        .with_product_id(product_id)
        .with_previous_billing_date(previous_billing_date)
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update subscription status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        UpdateBillingAccountStatusCommand::new(owner_id.clone(), status.to_string())
    .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update billing account status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        MarkCheckoutFlowFailedCommand::new(owner_id.clone(), "subscription.failed".to_string(), status.to_string())
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update checkout flow state: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!(
            "Subscription {} failed for owner {}",
            subscription_id, owner_id
        );
        send_billing_change_email(app_state, &owner_id, "Your subscription payment failed.").await;
    }

    Ok(())
}

pub(super) async fn handle_subscription_expired(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let product_id = data["product_id"].as_str().map(|s| s.to_string());
    let previous_billing_date = data["previous_billing_date"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let status = data["status"].as_str().unwrap_or("expired");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if !owner_id.is_empty() {
        UpsertSubscriptionCommand::new(
            next_subscription_id(app_state)?,
            owner_id.clone(),
            customer_id.to_string(),
            subscription_id.to_string(),
            status.to_string(),
        )
        .with_product_id(product_id)
        .with_previous_billing_date(previous_billing_date)
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update subscription status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        UpdateBillingAccountStatusCommand::new(owner_id.clone(), status.to_string())
    .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update billing account status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        MarkCheckoutFlowFailedCommand::new(owner_id.clone(), "subscription.expired".to_string(), status.to_string())
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update checkout flow state: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!(
            "Subscription {} expired for owner {}",
            subscription_id, owner_id
        );
        send_billing_change_email(app_state, &owner_id, "Your subscription has expired.").await;
    }

    Ok(())
}
