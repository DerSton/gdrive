use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{error, info, warn};

pub const OAUTH_REDIRECT_PORT: u16 = 53682;
pub const OAUTH_REDIRECT_URI: &str = "http://127.0.0.1:53682/";

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Erneute Anmeldung erforderlich (Token ungültig oder abgelaufen)")]
    NeedsReauth,
    #[error("Netzwerkfehler: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Anderer Fehler: {0}")]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RcloneToken {
    pub access_token: String,
    pub token_type: String,
    pub refresh_token: Option<String>,
    pub expiry: String, // RFC3339 format
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub fn build_auth_url(client_id: &str) -> String {
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap();
    url.query_pairs_mut()
        .append_pair("client_id", client_id.trim())
        .append_pair("redirect_uri", OAUTH_REDIRECT_URI)
        .append_pair("response_type", "code")
        .append_pair("scope", "https://www.googleapis.com/auth/drive")
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    url.to_string()
}

pub async fn run_oauth_flow(client_id: &str, client_secret: &str) -> Result<String> {
    let auth_url = build_auth_url(client_id);
    let addr: SocketAddr = format!("127.0.0.1:{}", OAUTH_REDIRECT_PORT).parse()?;
    let listener = TcpListener::bind(addr).await.with_context(|| {
        format!(
            "Konnte OAuth-Port {} nicht binden. Läuft rclone oder eine andere Instanz bereits?",
            OAUTH_REDIRECT_PORT
        )
    })?;

    info!("Öffne Standardbrowser für Google OAuth: {}", auth_url);
    if let Err(e) = opener::open_browser(&auth_url) {
        warn!("Konnte Browser nicht automatisch öffnen: {}. Bitte manuell aufrufen.", e);
    }

    info!("Warte auf Google-Callback auf Port {}...", OAUTH_REDIRECT_PORT);
    let mut code_opt = None;

    // Wait for the redirect request
    while code_opt.is_none() {
        let (mut socket, _) = listener.accept().await?;
        let mut buf = [0u8; 4096];
        let n = socket.read(&mut buf).await?;
        let req_str = String::from_utf8_lossy(&buf[..n]);

        if let Some(first_line) = req_str.lines().next() {
            let parts: Vec<&str> = first_line.split_whitespace().collect();
            if parts.len() >= 2 && parts[0] == "GET" {
                let path = parts[1];
                if let Ok(dummy_url) = url::Url::parse(&format!("http://localhost{}", path)) {
                    let params: HashMap<_, _> = dummy_url.query_pairs().into_owned().collect();
                    if let Some(code) = params.get("code") {
                        code_opt = Some(code.clone());

                        let html = r#"<!DOCTYPE html>
<html lang="de">
<head>
    <meta charset="utf-8">
    <title>Google Drive – Verbunden</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0f172a; color: #f8fafc; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }
        .card { background: #1e293b; padding: 40px; border-radius: 16px; text-align: center; box-shadow: 0 10px 25px rgba(0,0,0,0.5); border: 1px solid #334155; max-width: 460px; }
        h1 { color: #4ade80; margin-bottom: 12px; font-size: 24px; }
        p { color: #94a3b8; font-size: 15px; line-height: 1.5; }
        .badge { background: #064e3b; color: #6ee7b7; padding: 6px 12px; border-radius: 9999px; font-size: 13px; font-weight: 600; display: inline-block; margin-top: 15px; }
    </style>
</head>
<body>
    <div class="card">
        <h1>✅ Google Drive Verbunden</h1>
        <p>Die Autorisierung war erfolgreich. Dein Google Drive wird nun eingebunden.</p>
        <div class="badge">Du kannst dieses Browserfenster jetzt schließen.</div>
    </div>
</body>
</html>"#;
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            html.len(),
                            html
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        break;
                    }
                }
            }
        }

        let not_found = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let _ = socket.write_all(not_found.as_bytes()).await;
    }

    let code = match code_opt {
        Some(c) => c,
        None => bail!("Kein Autorisierungscode von Google empfangen"),
    };

    info!("Tausche Autorisierungscode gegen Tokens aus...");
    let client = reqwest::Client::new();
    let mut params = HashMap::new();
    params.insert("client_id", client_id.trim());
    params.insert("client_secret", client_secret.trim());
    params.insert("code", &code);
    params.insert("grant_type", "authorization_code");
    params.insert("redirect_uri", OAUTH_REDIRECT_URI);

    let res = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await?;

    if !res.status().is_success() {
        let err_text = res.text().await.unwrap_or_default();
        bail!("Google Token-Fehler: {}", err_text);
    }

    let token_resp: GoogleTokenResponse = res.json().await?;
    let expiry_time = SystemTime::now()
        + std::time::Duration::from_secs(token_resp.expires_in.saturating_sub(60));
    let expiry_rfc3339 = format_rfc3339(expiry_time);

    let rclone_tok = RcloneToken {
        access_token: token_resp.access_token,
        token_type: token_resp.token_type,
        refresh_token: token_resp.refresh_token,
        expiry: expiry_rfc3339,
    };

    let token_json = serde_json::to_string(&rclone_tok)?;
    info!("OAuth-Flow erfolgreich abgeschlossen!");
    Ok(token_json)
}

pub async fn refresh_token_if_needed(
    client_id: &str,
    client_secret: &str,
    token_json: &str,
) -> Result<Option<String>, AuthError> {
    let mut tok: RcloneToken = match serde_json::from_str(token_json) {
        Ok(t) => t,
        Err(e) => {
            error!("Fehler beim Parsen des Token-JSON: {}", e);
            return Err(AuthError::NeedsReauth);
        }
    };

    let now = SystemTime::now();
    let is_expired = match parse_rfc3339(&tok.expiry) {
        Some(exp) => {
            // Refresh if expiring within 5 minutes or already expired
            exp <= now + std::time::Duration::from_secs(300)
        }
        None => true,
    };

    if !is_expired {
        return Ok(None);
    }

    let refresh_token = match &tok.refresh_token {
        Some(rt) if !rt.trim().is_empty() => rt,
        _ => {
            warn!("Kein Refresh-Token vorhanden. Erneute Anmeldung nötig.");
            return Err(AuthError::NeedsReauth);
        }
    };

    info!("Access-Token abgelaufen, erneuere im Hintergrund...");
    let client = reqwest::Client::new();
    let mut params = HashMap::new();
    params.insert("client_id", client_id.trim());
    params.insert("client_secret", client_secret.trim());
    params.insert("refresh_token", refresh_token.as_str());
    params.insert("grant_type", "refresh_token");

    let res = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        warn!("Google Token Refresh fehlgeschlagen (Status {}): {}", status, body);
        if body.contains("invalid_grant") || status.as_u16() == 400 || status.as_u16() == 401 {
            return Err(AuthError::NeedsReauth);
        }
        return Err(AuthError::Other(anyhow::anyhow!(
            "Unerwarteter Fehler beim Token-Refresh: {}",
            body
        )));
    }

    let token_resp: GoogleTokenResponse = res.json().await.map_err(AuthError::Network)?;
    tok.access_token = token_resp.access_token;
    let expiry_time = SystemTime::now()
        + std::time::Duration::from_secs(token_resp.expires_in.saturating_sub(60));
    tok.expiry = format_rfc3339(expiry_time);

    let updated_json = serde_json::to_string(&tok).map_err(anyhow::Error::from)?;
    info!("Token erfolgreich im Hintergrund aktualisiert.");
    Ok(Some(updated_json))
}

fn format_rfc3339(t: SystemTime) -> String {
    let dur = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    // Rough RFC3339 calculation without chrono dependency
    let days = secs / 86400;
    let rem_secs = secs % 86400;
    let hours = rem_secs / 3600;
    let minutes = (rem_secs % 3600) / 60;
    let seconds = rem_secs % 60;

    // Approximate year/month/day
    let mut y = 1970i64;
    let mut d = days as i64;
    loop {
        let leap = if (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0) { 366 } else { 365 };
        if d < leap {
            break;
        }
        d -= leap;
        y += 1;
    }

    let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
    let month_days = [
        31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31
    ];
    let mut m = 0;
    for (idx, &md) in month_days.iter().enumerate() {
        if d < md {
            m = idx + 1;
            break;
        }
        d -= md;
    }
    let day = d + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, day, hours, minutes, seconds
    )
}

fn parse_rfc3339(s: &str) -> Option<SystemTime> {
    // Basic parser for "YYYY-MM-DDTHH:MM:SS"
    if s.len() < 19 {
        return None;
    }
    let y: u64 = s[0..4].parse().ok()?;
    let m: u64 = s[5..7].parse().ok()?;
    let d: u64 = s[8..10].parse().ok()?;
    let h: u64 = s[11..13].parse().ok()?;
    let min: u64 = s[14..16].parse().ok()?;
    let sec: u64 = s[17..19].parse().ok()?;

    let mut days = 0u64;
    for year in 1970..y {
        let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        days += if leap { 366 } else { 365 };
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
    let month_days = [
        31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31
    ];
    for month in 1..m {
        days += month_days[(month - 1) as usize];
    }
    days += d - 1;

    let total_secs = days * 86400 + h * 3600 + min * 60 + sec;
    Some(UNIX_EPOCH + std::time::Duration::from_secs(total_secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_auth_url() {
        let url_str = build_auth_url("my-client-id.apps.googleusercontent.com");
        assert!(url_str.contains("client_id=my-client-id.apps.googleusercontent.com"));
        assert!(url_str.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A53682%2F"));
        assert!(url_str.contains("response_type=code"));
        assert!(url_str.contains("access_type=offline"));
        assert!(url_str.contains("prompt=consent"));
    }

    #[test]
    fn test_rfc3339_roundtrip() {
        let now = UNIX_EPOCH + std::time::Duration::from_secs(1726135200); // 2024-09-12T10:00:00Z
        let formatted = format_rfc3339(now);
        let parsed = parse_rfc3339(&formatted).expect("Should parse");
        assert_eq!(now, parsed);
    }

    #[test]
    fn test_rclone_token_deserialize() {
        let json = r#"{"access_token":"ya29.test","token_type":"Bearer","refresh_token":"1//test","expiry":"2026-09-12T12:00:00Z"}"#;
        let token: RcloneToken = serde_json::from_str(json).unwrap();
        assert_eq!(token.access_token, "ya29.test");
        assert_eq!(token.refresh_token.as_deref(), Some("1//test"));
    }
}

