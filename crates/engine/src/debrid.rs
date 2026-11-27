//! Debrid accounts: a file host link is resolved through the user's own
//! account, so the download comes from the debrid server at full speed,
//! without the host's free-user limits.
//!
//! Real-Debrid and AllDebrid share the same shape: ask who the token is,
//! then unlock the link. TorBox is a different model: its servers fetch the
//! file first, so the download is created there, waited for, and then its
//! direct link is asked for (see `torbox_fetch`).

use anyhow::{anyhow, bail, Context, Result};
use std::time::Duration;

/// The providers this program knows: the settings store one of these names.
pub const PROVIDERS: &[&str] = &["realdebrid", "alldebrid", "torbox"];

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
        "torbox" => {
            let r = client
                .get("https://api.torbox.app/v1/api/user/me")
                .header("Authorization", format!("Bearer {token}"))
                .timeout(Duration::from_secs(15))
                .send()
                .await
                .context("TorBox did not answer")?;
            let status = r.status();
            let text = r.text().await.unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            if !status.is_success() || v["success"].as_bool() == Some(false) {
                let why = v["detail"].as_str().or(v["error"].as_str()).unwrap_or("token refused");
                bail!("TorBox refused the token: {why}");
            }
            let d = &v["data"];
            Ok(d["email"].as_str().or(d["username"].as_str()).unwrap_or("account").to_string())
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

/// TorBox: its servers fetch the file first. Create the download there, wait
/// until it is ready, then ask for the direct link.
pub async fn torbox_fetch(client: &reqwest::Client, token: &str, url: &str, wait: Duration) -> Result<String> {
    match torbox_one(client, token, url, wait).await {
        Ok(link) => Ok(link),
        Err(first) => {
            // Some hosts' links carry a tracking part ("?short_domain=...")
            // that breaks TorBox's own fetcher: the bare address works.
            let bare = url.split(['?', '#']).next().unwrap_or(url);
            if bare == url || bare.is_empty() {
                return Err(first);
            }
            match torbox_one(client, token, bare, wait).await {
                Ok(link) => Ok(link),
                Err(second) => Err(anyhow!("{second:#} (and without the query: {first:#})")),
            }
        }
    }
}

/// One TorBox web download: create, wait, direct link.
async fn torbox_one(client: &reqwest::Client, token: &str, url: &str, wait: Duration) -> Result<String> {
    let auth = format!("Bearer {token}");
    let r = client
        .post("https://api.torbox.app/v1/api/webdl/createwebdownload")
        .header("Authorization", &auth)
        .form(&[("link", url)])
        .timeout(Duration::from_secs(25))
        .send()
        .await
        .context("TorBox did not answer")?;
    let status = r.status();
    let text = r.text().await.unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    if !status.is_success() || v["success"].as_bool() == Some(false) {
        bail!(
            "TorBox refused the link: {}",
            v["detail"].as_str().or(v["error"].as_str()).unwrap_or("refused")
        );
    }
    let d = &v["data"];
    let id = d["webdownload_id"]
        .as_u64()
        .or_else(|| d["id"].as_u64())
        .or_else(|| d.as_u64())
        .context("TorBox did not say which download it made")?;

    let started = std::time::Instant::now();
    let file_id = loop {
        if started.elapsed() > wait {
            bail!("TorBox is still fetching this file; try again in a minute");
        }
        let r = client
            .get("https://api.torbox.app/v1/api/webdl/mylist")
            .query(&[("id", id.to_string()), ("bypass_cache", "true".to_string())])
            .header("Authorization", &auth)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .context("TorBox did not answer")?;
        let text = r.text().await.unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let item = match &v["data"] {
            serde_json::Value::Array(a) => a.first(),
            other if other.is_object() => Some(other),
            _ => None,
        };
        if let Some(item) = item {
            let state = item["download_state"].as_str().unwrap_or("");
            // TorBox writes more after the word: "failed (Turbobit Download)"
            let lower = state.to_ascii_lowercase();
            if lower.starts_with("completed") {
                let file = item["files"].as_array().and_then(|f| f.first()).context("TorBox returned no file")?;
                break file["id"].as_u64().context("TorBox returned no file id")?;
            }
            if lower.starts_with("failed") || lower.starts_with("error") || lower.starts_with("cancelled") {
                bail!("TorBox could not fetch this link ({state})");
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    };

    let r = client
        .get("https://api.torbox.app/v1/api/webdl/requestdl")
        .query(&[
            ("token", token.to_string()),
            ("web_id", id.to_string()),
            ("file_id", file_id.to_string()),
            ("redirect", "false".to_string()),
        ])
        .timeout(Duration::from_secs(25))
        .send()
        .await
        .context("TorBox did not answer")?;
    let text = r.text().await.unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    v["data"]
        .as_str()
        .filter(|u| u.starts_with("http"))
        .map(str::to_string)
        .with_context(|| format!("TorBox returned no link: {}", v["detail"].as_str().unwrap_or("")))
}
