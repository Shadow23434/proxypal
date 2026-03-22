//! Auth & OAuth commands.
//!
//! Extracted from lib.rs — handles authentication status, OAuth flows,
//! provider connection/disconnection, and credential management.

use crate::state::AppState;
use crate::types::{AuthStatus, OAuthState, ProxyAuthProviderStatus};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};
use tauri_plugin_opener::OpenerExt;

/// OAuth URL response for frontend modal
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OAuthUrlResponse {
    pub url: String,
    pub state: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeResponse {
    pub verification_uri: String,
    pub user_code: String,
    pub state: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[tauri::command]
pub fn get_auth_status(state: State<AppState>) -> AuthStatus {
    state.auth_status.lock().unwrap().clone()
}

fn provider_count_from_proxy(status: Option<&ProxyAuthProviderStatus>) -> Option<u32> {
    status.map(|provider| match provider.accounts {
        Some(accounts) if accounts > 0 => accounts,
        _ if provider.authenticated => 1,
        _ => 0,
    })
}

fn overlay_proxy_auth_counts(auth: &mut AuthStatus, proxy_status: &crate::types::ProxyAuthStatus) {
    let providers = &proxy_status.providers;

    if let Some(count) = provider_count_from_proxy(providers.claude.as_ref()) {
        auth.claude = count;
    }
    if let Some(count) = provider_count_from_proxy(providers.openai.as_ref()) {
        auth.openai = count;
    }
    if let Some(count) = provider_count_from_proxy(providers.gemini.as_ref()) {
        auth.gemini = count;
    }
    if let Some(count) = provider_count_from_proxy(providers.gemini_web.as_ref()) {
        auth.gemini_web = count;
    }
    if let Some(count) = provider_count_from_proxy(providers.qwen.as_ref()) {
        auth.qwen = count;
    }
    if let Some(count) = provider_count_from_proxy(providers.iflow.as_ref()) {
        auth.iflow = count;
    }
    if let Some(count) = provider_count_from_proxy(providers.vertex.as_ref()) {
        auth.vertex = count;
    }
    if let Some(count) = provider_count_from_proxy(providers.kiro.as_ref()) {
        auth.kiro = count;
    }
    if let Some(count) = provider_count_from_proxy(providers.antigravity.as_ref()) {
        auth.antigravity = count;
    }
    if let Some(count) = provider_count_from_proxy(providers.kimi.as_ref()) {
        auth.kimi = count;
    }
}

/// Get OAuth URL without opening browser (for modal flow)
#[tauri::command]
pub async fn get_oauth_url(
    state: State<'_, AppState>,
    provider: String,
) -> Result<OAuthUrlResponse, String> {
    // Get proxy port from config
    let port = {
        let config = state.config.lock().unwrap();
        config.port
    };

    // Kiro uses a web UI page directly, not a JSON API endpoint
    // Return the URL directly without making an HTTP request
    if provider == "kiro" {
        let kiro_url = format!("http://127.0.0.1:{}/v0/oauth/kiro", port);
        return Ok(OAuthUrlResponse {
            url: kiro_url,
            state: String::new(),
        });
    }

    // Get the OAuth URL from CLIProxyAPI's Management API
    // Add is_webui=true to use the embedded callback forwarder
    // Use 127.0.0.1 consistently (not localhost) to avoid access control issues
    let endpoint = match provider.as_str() {
        "claude" => format!(
            "http://127.0.0.1:{}/v0/management/anthropic-auth-url?is_webui=true",
            port
        ),
        "openai" => format!(
            "http://127.0.0.1:{}/v0/management/codex-auth-url?is_webui=true",
            port
        ),
        "gemini" => format!(
            "http://127.0.0.1:{}/v0/management/gemini-cli-auth-url?is_webui=true",
            port
        ),
        "gemini-web" => {
            return Err(
                "Gemini Web now uses cookie-based login. Use create_gemini_web_token instead."
                    .to_string(),
            )
        }
        "qwen" => format!(
            "http://127.0.0.1:{}/v0/management/qwen-auth-url?is_webui=true",
            port
        ),
        "iflow" => format!(
            "http://127.0.0.1:{}/v0/management/iflow-auth-url?is_webui=true",
            port
        ),
        "antigravity" => format!(
            "http://127.0.0.1:{}/v0/management/antigravity-auth-url?is_webui=true",
            port
        ),
        "kimi" => format!(
            "http://127.0.0.1:{}/v0/management/kimi-auth-url?is_webui=true",
            port
        ),
        "vertex" => {
            return Err(
                "Vertex uses service account import, not OAuth. Use import_vertex_credential instead."
                    .to_string(),
            )
        }
        _ => return Err(format!("Unknown provider: {}", provider)),
    };

    // Make HTTP request to get OAuth URL
    let client = crate::build_management_client();
    let response = client
        .get(&endpoint)
        .header("X-Management-Key", &crate::get_management_key())
        .send()
        .await
        .map_err(|e| format!("Failed to get OAuth URL: {}. Is the proxy running?", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Management API returned error: {}",
            response.status()
        ));
    }

    // Parse response to get URL and state
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let oauth_url = body["url"]
        .as_str()
        .ok_or("No URL in response")?
        .to_string();

    let oauth_state = body["state"].as_str().unwrap_or("").to_string();

    // Store pending OAuth state
    {
        let mut pending = state.pending_oauth.lock().unwrap();
        *pending = Some(OAuthState {
            provider: provider.clone(),
            state: oauth_state.clone(),
        });
    }

    Ok(OAuthUrlResponse {
        url: oauth_url,
        state: oauth_state,
    })
}

#[tauri::command]
pub async fn get_device_code(
    state: State<'_, AppState>,
    provider: String,
) -> Result<DeviceCodeResponse, String> {
    // Get the proxy port from config
    let port = {
        let config = state.config.lock().map_err(|e| e.to_string())?;
        config.port
    };

    // Build endpoint WITHOUT ?is_webui=true to trigger device-code flow
    let endpoint = match provider.as_str() {
        "openai" => format!(
            "http://127.0.0.1:{}/v0/management/codex-auth-url",
            port
        ),
        "qwen" => format!(
            "http://127.0.0.1:{}/v0/management/qwen-auth-url",
            port
        ),
        _ => return Err(format!("Device code flow not supported for provider: {}", provider)),
    };

    let client = crate::build_management_client();
    let response = client
        .get(&endpoint)
        .header("X-Management-Key", &crate::get_management_key())
        .send()
        .await
        .map_err(|e| format!("Failed to get device code: {}. Is the proxy running?", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Device code request failed ({}): {}",
            status, body
        ));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse device code response: {}", e))?;

    let verification_uri = body["verification_uri"]
        .as_str()
        .or_else(|| body["verification_url"].as_str())
        .or_else(|| body["url"].as_str())
        .ok_or("Missing verification_uri in response")?
        .to_string();

    let user_code = body["user_code"].as_str().unwrap_or("").to_string();

    let oauth_state = body["state"]
        .as_str()
        .or_else(|| body["device_code"].as_str())
        .unwrap_or("")
        .to_string();

    let expires_in = body["expires_in"].as_u64().unwrap_or(900);
    let interval = body["interval"].as_u64().unwrap_or(5);

    // Store pending OAuth state for callback matching
    {
        let mut pending = state.pending_oauth.lock().map_err(|e| e.to_string())?;
        *pending = Some(OAuthState {
            provider: provider.clone(),
            state: oauth_state.clone(),
        });
    }

    Ok(DeviceCodeResponse {
        verification_uri,
        user_code,
        state: oauth_state,
        expires_in,
        interval,
    })
}

/// Open a URL in the default browser
#[tauri::command]
pub async fn open_url_in_browser(app: tauri::AppHandle, url: String) -> Result<(), String> {
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn open_oauth(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    provider: String,
) -> Result<String, String> {
    // Get proxy port from config
    let port = {
        let config = state.config.lock().unwrap();
        config.port
    };

    // For Kiro, open the Web OAuth UI directly in CLIProxyAPIPlus
    if provider == "kiro" {
        let oauth_url = format!("http://127.0.0.1:{}/v0/oauth/kiro", port);
        app.opener()
            .open_url(&oauth_url, None::<&str>)
            .map_err(|e| format!("Failed to open URL: {}", e))?;
        return Ok(String::new()); // No specific state needed for direct Web UI
    }

    // Get the OAuth URL from CLIProxyAPI's Management API
    // Add is_webui=true to use the embedded callback forwarder
    // Use 127.0.0.1 consistently (not localhost) to avoid access control issues
    let endpoint = match provider.as_str() {
        "claude" => format!(
            "http://127.0.0.1:{}/v0/management/anthropic-auth-url?is_webui=true",
            port
        ),
        "openai" => format!(
            "http://127.0.0.1:{}/v0/management/codex-auth-url?is_webui=true",
            port
        ),
        "gemini" => format!(
            "http://127.0.0.1:{}/v0/management/gemini-cli-auth-url?is_webui=true",
            port
        ),
        "gemini-web" => {
            return Err(
                "Gemini Web now uses cookie-based login. Use create_gemini_web_token instead."
                    .to_string(),
            )
        }
        "qwen" => format!(
            "http://127.0.0.1:{}/v0/management/qwen-auth-url?is_webui=true",
            port
        ),
        "iflow" => format!(
            "http://127.0.0.1:{}/v0/management/iflow-auth-url?is_webui=true",
            port
        ),
        "antigravity" => format!(
            "http://127.0.0.1:{}/v0/management/antigravity-auth-url?is_webui=true",
            port
        ),
        "kimi" => format!(
            "http://127.0.0.1:{}/v0/management/kimi-auth-url?is_webui=true",
            port
        ),
        "vertex" => {
            return Err(
                "Vertex uses service account import, not OAuth. Use import_vertex_credential instead."
                    .to_string(),
            )
        }
        // Note: Kiro is handled above with direct Web UI
        _ => return Err(format!("Unknown provider: {}", provider)),
    };

    // Make HTTP request to get OAuth URL
    let client = crate::build_management_client();
    let response = client
        .get(&endpoint)
        .header("X-Management-Key", &crate::get_management_key())
        .send()
        .await
        .map_err(|e| format!("Failed to get OAuth URL: {}. Is the proxy running?", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Management API returned error: {}",
            response.status()
        ));
    }

    // Parse response to get URL and state
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let oauth_url = body["url"]
        .as_str()
        .ok_or("No URL in response")?
        .to_string();

    let oauth_state = body["state"].as_str().unwrap_or("").to_string();

    // Store pending OAuth state
    {
        let mut pending = state.pending_oauth.lock().unwrap();
        *pending = Some(OAuthState {
            provider: provider.clone(),
            state: oauth_state.clone(),
        });
    }

    // Open the OAuth URL in the default browser
    app.opener()
        .open_url(&oauth_url, None::<&str>)
        .map_err(|e| e.to_string())?;

    // Return the state so frontend can poll for completion
    Ok(oauth_state)
}

#[tauri::command]
pub async fn poll_oauth_status(
    state: State<'_, AppState>,
    oauth_state: String,
) -> Result<bool, String> {
    let port = {
        let config = state.config.lock().unwrap();
        config.port
    };

    let endpoint = format!(
        "http://localhost:{}/v0/management/get-auth-status?state={}",
        port, oauth_state
    );

    let client = crate::build_management_client();
    let response = client
        .get(&endpoint)
        .header("X-Management-Key", &crate::get_management_key())
        .send()
        .await
        .map_err(|e| format!("Failed to poll OAuth status: {}", e))?;

    if !response.status().is_success() {
        return Ok(false); // Not ready yet
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    // Check if auth is complete - CLIProxyAPI returns { "status": "ok" } when done
    let status = body["status"].as_str().unwrap_or("wait");
    Ok(status == "ok")
}

#[tauri::command]
pub async fn refresh_auth_status(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<AuthStatus, String> {
    // Check CLIProxyAPI's auth directory for credentials
    let auth_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".cli-proxy-api");

    let mut new_auth = AuthStatus::default();

    // Scan auth directory for credential files and count them per provider
    if let Ok(entries) = std::fs::read_dir(&auth_dir) {
        for entry in entries.flatten() {
            let filename = entry.file_name().to_string_lossy().to_lowercase();

            // CLIProxyAPI naming patterns:
            // - claude-{email}.json or anthropic-*.json
            // - codex-{email}.json
            // - gemini-{email}-{project}.json
            // - gemini-web-{email}-{project}.json
            // - qwen-{email}.json
            // - iflow-{email}.json
            // - vertex-{project_id}.json
            // - antigravity-{email}.json

            if filename.ends_with(".json") {
                if filename.starts_with("claude-") || filename.starts_with("anthropic-") {
                    new_auth.claude += 1;
                } else if filename.starts_with("codex-") {
                    new_auth.openai += 1;
                } else if filename.starts_with("gemini-web-") {
                    new_auth.gemini_web += 1;
                } else if filename.starts_with("gemini-") {
                    new_auth.gemini += 1;
                } else if filename.starts_with("qwen-") {
                    new_auth.qwen += 1;
                } else if filename.starts_with("iflow-") {
                    new_auth.iflow += 1;
                } else if filename.starts_with("vertex-") {
                    new_auth.vertex += 1;
                } else if filename.starts_with("kiro-") {
                    new_auth.kiro += 1;
                } else if filename.starts_with("antigravity-") {
                    new_auth.antigravity += 1;
                } else if filename.starts_with("kimi-") {
                    new_auth.kimi += 1;
                }
            }
        }
    }

    if let Ok(proxy_status) = crate::commands::auth_files::verify_proxy_auth_status(state.clone()).await {
        if proxy_status.status != "unsupported" {
            overlay_proxy_auth_counts(&mut new_auth, &proxy_status);
        }
    }

    // Update state
    {
        let mut auth = state.auth_status.lock().unwrap();
        *auth = new_auth.clone();
    }

    // Save to our config
    crate::save_auth_to_file(&new_auth)?;

    // Emit auth status update
    let _ = app.emit("auth-status-changed", new_auth.clone());

    Ok(new_auth)
}

#[tauri::command]
pub async fn complete_oauth(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    provider: String,
    code: String,
) -> Result<AuthStatus, String> {
    let _ = code;

    match provider.as_str() {
        "claude" | "openai" | "gemini" | "gemini-web" | "qwen" | "iflow" | "vertex"
        | "kiro" | "antigravity" | "kimi" => {}
        _ => return Err(format!("Unknown provider: {}", provider)),
    }

    {
        let mut pending = state.pending_oauth.lock().unwrap();
        *pending = None;
    }

    refresh_auth_status(app, state).await
}

#[tauri::command]
pub async fn disconnect_provider(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    provider: String,
) -> Result<AuthStatus, String> {
    // Delete credential files from ~/.cli-proxy-api/ for this provider
    let auth_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".cli-proxy-api");

    if auth_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&auth_dir) {
            for entry in entries.flatten() {
                let filename = entry.file_name().to_string_lossy().to_lowercase();

                // Match credential files by provider prefix
                let should_delete = match provider.as_str() {
                    "claude" => {
                        filename.starts_with("claude-") || filename.starts_with("anthropic-")
                    }
                    "openai" => filename.starts_with("codex-"),
                    "gemini" => {
                        filename.starts_with("gemini-") && !filename.starts_with("gemini-web-")
                    }
                    "gemini-web" => filename.starts_with("gemini-web-"),
                    "qwen" => filename.starts_with("qwen-"),
                    "iflow" => filename.starts_with("iflow-"),
                    "vertex" => filename.starts_with("vertex-"),
                    "kiro" => filename.starts_with("kiro-"),
                    "antigravity" => filename.starts_with("antigravity-"),
                    "kimi" => filename.starts_with("kimi-"),
                    _ => false,
                };

                if should_delete && filename.ends_with(".json") {
                    if let Err(e) = std::fs::remove_file(entry.path()) {
                        eprintln!("Failed to delete credential file {:?}: {}", entry.path(), e);
                    }
                }
            }
        }
    }

    let mut auth = state.auth_status.lock().unwrap();

    match provider.as_str() {
        "claude" => auth.claude = 0,
        "openai" => auth.openai = 0,
        "gemini" => auth.gemini = 0,
        "gemini-web" => auth.gemini_web = 0,
        "qwen" => auth.qwen = 0,
        "iflow" => auth.iflow = 0,
        "vertex" => auth.vertex = 0,
        "kiro" => auth.kiro = 0,
        "antigravity" => auth.antigravity = 0,
        "kimi" => auth.kimi = 0,
        _ => return Err(format!("Unknown provider: {}", provider)),
    }

    // Save to file
    crate::save_auth_to_file(&auth)?;

    // Emit auth status update
    let _ = app.emit("auth-status-changed", auth.clone());

    Ok(auth.clone())
}
