use anyhow::Result;
use celery::prelude::*;
use dotenvy::dotenv;
use std::env;
use tracing::Level;
use tracing_subscriber;

mod tasks;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let redis_url = env::var("REDIS_URL").unwrap();

    let app = celery::app!(
        broker = RedisBroker { &redis_url },
        tasks = [
            tasks::token::clean_token,
            tasks::email::send_verification_email,
            tasks::email::send_password_reset_email,
            tasks::email::send_magic_link_email,
            tasks::email::send_signin_notification_email,
            tasks::email::send_email_change_notification,
            tasks::email::send_password_change_notification,
            tasks::email::send_password_remove_notification,
            tasks::email::send_waitlist_signup_email,
            tasks::email::send_organization_membership_invite,
            tasks::email::send_deployment_invite,
            tasks::email::send_waitlist_approval,
            tasks::sms::send_sms,
        ],
        task_routes = [
            "*" => "worker_queue",
        ],
        prefetch_count = 2,
        heartbeat = Some(10),
    ).await?;

    app.consume().await?;

    Ok(())
}
