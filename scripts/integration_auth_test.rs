use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use tokio;

#[tokio::main]
async fn main() {
    let endpoints = [
        "/metrics",
        "/health",
        "/api/admin/info",
        "/api/plugins/list",
    ];
    let base_url = "http://localhost:9000";
    let admin_jwt = std::env::var("ADMIN_JWT").unwrap_or_default();
    let partner_jwt = std::env::var("PARTNER_JWT").unwrap_or_default();
    let user_jwt = std::env::var("USER_JWT").unwrap_or_default();

    for endpoint in endpoints.iter() {
        for (role, jwt) in &[ ("Admin", &admin_jwt), ("Partner", &partner_jwt), ("User", &user_jwt) ] {
            let url = format!("{}{}", base_url, endpoint);
            let mut headers = HeaderMap::new();
            if !jwt.is_empty() {
                headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", jwt)).unwrap());
            }
            let client = reqwest::Client::new();
            let resp = client.get(&url)
                .headers(headers.clone())
                .send().await;
            match resp {
                Ok(r) => {
                    let status = r.status();
                    let body = r.text().await.unwrap_or_default();
                    println!("[{}] {} => {}\n{}\n", role, endpoint, status, body);
                },
                Err(e) => {
                    println!("[{}] {} => ERROR: {}", role, endpoint, e);
                }
            }
        }
    }
} 