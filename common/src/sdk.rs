pub struct CommonClient {
    base_url: String,
    http: reqwest::Client,
}

impl CommonClient {
    pub async fn verify_token(&self, token: &str) -> anyhow::Result<bool> {
        let resp = self.http
            .post(format!("{}/auth/verify", self.base_url))
            .json(&serde_json::json!({ "token": token }))
            .send()
            .await?;

        Ok(resp.status().is_success())
    }
}
