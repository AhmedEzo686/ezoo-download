//! Debrid accounts: a file host link is resolved through the user's own
//! account, so the download comes from the debrid server at full speed,
//! without the host's free-user limits.
//!
//! Real-Debrid and AllDebrid share the same shape: ask who the token is,
//! then unlock the link. TorBox is a different model (the file is fetched by
//! their servers first), so it is not here yet.

use anyhow::{bail, Context, Result};
use std::time::Duration;

/// The providers this program knows: the settings store one of these names.
pub const PROVIDERS: &[&str] = &["realdebrid", "alldebrid"];

pub fn known(provider: &str) -> bool {
    PROVIDERS.contains(&provider)
}

/// Ask the account who it is: a wrong or expired token is refused.
pub async fn check(client: &reqwest::Client, provider: &str, token: &str) -> Result<String> {
    match provider {
        "realdebrid" => {
            let r = client
                .get("https://api.real-debrid.com/rest/1.0/user")
                .header("Authorization", format!("Bearer {token}"))
                .timeout(Duration::from_secs(15))
                .send()
                .await
                .context("Real-Debrid did not answer")?;
            if !r.status().is_success() {
                bail!("the token was refused (HTTP {})", r.status().as_u16());
            }
            let text = r.text().await.unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            Ok(v["username"].as_str().unwrap_or("account").to_string())
        }
        "alldebrid" => {
            let r = client
                .get("https://api.alldebrid.com/v4.1/user?agent=ezoo")
                .header("Authorization", format!("Bearer {token}"))
                .timeout(Duration::from_secs(15))
                .send()
                .await
                .context("AllDebrid did not answer")?;
            let status = r.status();
            let text = r.text().await.unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            if !status.is_success() || v["status"].as_str() == Some("error") {
                let why = v["error"]["message"].as_str().unwrap_or("token refused");
                bail!("AllDebrid refused the token: {why}");
            }
            Ok(v["data"]["user"]["username"].as_str().unwrap_or("account").to_string())
        }
        other => bail!("unknown provider: {other}"),
    }
}

/// The direct link behind a host link, through the account.
pub async fn unrestrict(client: &reqwest::Client, provider: &str, token: &str, url: &str) -> Result<String> {
    match provider {
        "realdebrid" => {
            let r = client
                .post("https://api.real-debrid.com/rest/1.0/unrestrict/link")
                .header("Authorization", format!("Bearer {token}"))
                .form(&[("link", url)])
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .context("Real-Debrid did not answer")?;
            let status = r.status();
            let text = r.text().await.unwrap_or_default();
            let body: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            if !status.is_success() {
                let why = body["error"].as_str().or_else(|| body["error_code"].as_str()).unwrap_or("refused");
                bail!("Real-Debrid refused the link: {why}");
            }
            body["download"]
                .as_str()
                .filter(|d| d.starts_with("http"))
                .map(str::to_string)
                .context("Real-Debrid returned no download link")
        }
        "alldebrid" => {
            let r = client
                .post("https://api.alldebrid.com/v4.1/link/unlock?agent=ezoo")
                .header("Authorization", format!("Bearer {token}"))
                .form(&[("link", url)])
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .context("AllDebrid did not answer")?;
            let status = r.status();
            let text = r.text().await.unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            if !status.is_success() || v["status"].as_str() == Some("error") {
                let why = v["error"]["message"].as_str().unwrap_or("refused");
                bail!("AllDebrid refused the link: {why}");
            }
            v["data"]["link"]
                .as_str()
                .filter(|d| d.starts_with("http"))
                .map(str::to_string)
                .context("AllDebrid returned no download link")
        }
        other => bail!("unknown provider: {other}"),
    }
}
