use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct WebhookDispatcher {
    db: hiqlite::Client,
    client: reqwest::Client,
}

impl WebhookDispatcher {
    pub fn new(db: hiqlite::Client) -> Self {
        Self {
            db,
            client: reqwest::Client::new(),
        }
    }

    /// Finds all active webhooks on `repo_id` subscribed to `event_name` and
    /// fires an async HTTP POST delivery for each. Individual delivery
    /// failures are logged but never propagated to the caller.
    pub async fn dispatch(
        &self,
        repo_id: Uuid,
        event_name: &str,
        payload: serde_json::Value,
    ) -> anyhow::Result<()> {
        let webhooks = self
            .db
            .query_as::<entity::webhook::Model, _>(
                "SELECT * FROM webhooks WHERE repo_id = ?1 AND active = 1",
                hiqlite::params!(repo_id.to_string()),
            )
            .await?;

        let body = serde_json::json!({
            "event": event_name,
            "payload": payload,
        });
        let body_bytes = serde_json::to_vec(&body)?;

        for webhook in webhooks {
            let subscribed = serde_json::from_str::<Vec<String>>(&webhook.events)
                .map(|arr| arr.iter().any(|v| v == event_name))
                .unwrap_or(false);
            if !subscribed {
                continue;
            }

            let client = self.client.clone();
            let target_url = webhook.target_url.clone();
            let secret = webhook.secret.clone();
            let event_name = event_name.to_string();
            let body_bytes = body_bytes.clone();

            tokio::spawn(async move {
                let signature = match HmacSha256::new_from_slice(secret.as_bytes()) {
                    Ok(mut mac) => {
                        mac.update(&body_bytes);
                        hex::encode(mac.finalize().into_bytes())
                    }
                    Err(e) => {
                        tracing::warn!("failed to compute webhook signature: {e}");
                        return;
                    }
                };

                let result = client
                    .post(&target_url)
                    .header("Content-Type", "application/json")
                    .header("X-Genome-Event", &event_name)
                    .header(
                        "X-Genome-Signature-256",
                        format!("sha256={signature}"),
                    )
                    .body(body_bytes)
                    .send()
                    .await;

                match result {
                    Ok(resp) if resp.status().is_success() => {
                        tracing::info!(
                            "webhook delivery to {target_url} for event {event_name} succeeded"
                        );
                    }
                    Ok(resp) => {
                        tracing::warn!(
                            "webhook delivery to {target_url} for event {event_name} returned status {}",
                            resp.status()
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "webhook delivery to {target_url} for event {event_name} failed: {e}"
                        );
                    }
                }
            });
        }

        Ok(())
    }
}
