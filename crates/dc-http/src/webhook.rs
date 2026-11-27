//! A post through a webhook: the address carries a secret, so this path sends no account header and keeps the
//! address out of every log line and error.

use super::{Client, Error};
use dc_proto::pin;
use serde_json::Value;

impl Client {
    /// `POST /webhooks/{id}/{token}?wait=true&with_components=true` with `payload` as the JSON body, or as the
    /// `payload_json` part beside one attached file. Answers the created message.
    pub async fn post_webhook(
        &self,
        id: &str,
        secret: &str,
        payload: &Value,
        file: Option<(&str, Vec<u8>)>,
    ) -> Result<Value, Error> {
        let url = format!(
            "{}/webhooks/{id}/{secret}?wait=true&with_components=true",
            pin::API_BASE
        );
        // A plain client: the account client carries a browser identity, which a webhook refuses (50067).
        let plain = reqwest::Client::builder()
            .user_agent("gravecord-fixture")
            .build()
            .map_err(|error| Error::Transport(super::Transport::Plain(error.without_url())))?;
        let request = plain.post(url);
        let request = match file {
            None => request.json(payload),
            Some((name, bytes)) => request.multipart(
                reqwest::multipart::Form::new()
                    .text("payload_json", payload.to_string())
                    .part(
                        "files[0]",
                        reqwest::multipart::Part::bytes(bytes).file_name(name.to_owned()),
                    ),
            ),
        };
        let response = request
            .send()
            .await
            .map_err(|error| Error::Transport(super::Transport::Plain(error.without_url())))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| Error::Transport(super::Transport::Plain(error.without_url())))?;
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() {
            Ok(value)
        } else {
            Err(Error::Status {
                route: "/webhooks".to_owned(),
                status,
                body: value,
            })
        }
    }
}
