use crate::config::save_config_to_file;
use crate::state::AppState;
use crate::types::{AvailableModel, ProviderTestResult};
use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;
use tauri::State;

// Internal types for model API responses
#[derive(Debug, Deserialize)]
struct ModelsApiResponse {
    data: Vec<ModelsApiModel>,
}

#[derive(Debug, Deserialize)]
struct ModelsApiModel {
    id: String,
    owned_by: String,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaTagModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagModel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIErrorResponse {
    error: Option<OpenAIErrorBody>,
}

#[derive(Debug, Deserialize)]
struct OpenAIErrorBody {
    message: Option<String>,
    r#type: Option<String>,
    code: Option<serde_json::Value>,
}

fn truncate_message(message: &str, max_chars: usize) -> String {
    let trimmed = message.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let truncated: String = trimmed.chars().take(max_chars).collect();
    format!("{}…", truncated.trim_end())
}

fn format_openai_error(status: StatusCode, body: &str) -> String {
    let parsed = serde_json::from_str::<OpenAIErrorResponse>(body)
        .ok()
        .and_then(|json| json.error);

    if let Some(error) = parsed {
        let message = error
            .message
            .filter(|value| !value.trim().is_empty())
            .map(|value| truncate_message(&value, 220));
        let error_type = error.r#type.filter(|value| !value.trim().is_empty());
        let error_code = error
            .code
            .and_then(|value| match value {
                serde_json::Value::Null => None,
                serde_json::Value::String(text) => Some(text),
                other => Some(other.to_string()),
            })
            .filter(|value| !value.trim().is_empty());

        let mut details = Vec::new();
        if let Some(error_type) = error_type {
            details.push(format!("type {}", error_type));
        }
        if let Some(error_code) = error_code {
            details.push(format!("code {}", error_code));
        }

        if let Some(message) = message {
            if details.is_empty() {
                return format!("Request failed ({}): {}", status, message);
            }
            return format!("Request failed ({}): {} [{}]", status, message, details.join(", "));
        }
    }

    let compact_body = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact_body.is_empty() {
        return format!("Request failed ({})", status);
    }

    format!(
        "Request failed ({}): {}",
        status,
        truncate_message(&compact_body, 220)
    )
}

fn is_completion_fallback_candidate(status: StatusCode, body: &str) -> bool {
    if !(status == StatusCode::BAD_REQUEST
        || status == StatusCode::NOT_FOUND
        || status == StatusCode::METHOD_NOT_ALLOWED
        || status == StatusCode::UNPROCESSABLE_ENTITY)
    {
        return false;
    }

    let normalized = body.to_ascii_lowercase();
    [
        "chat/completions",
        "completions endpoint",
        "messages",
        "prompt",
        "unsupported endpoint",
        "unsupported path",
        "not found",
        "ollama",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn normalize_base_url(base_url: &str) -> (&str, &str) {
    let base_url = base_url.trim_end_matches('/');
    let root_url = base_url.strip_suffix("/v1").unwrap_or(base_url);
    (base_url, root_url)
}

fn build_openai_model_endpoints(base_url: &str) -> Vec<String> {
    let (base_url, root_url) = normalize_base_url(base_url);

    let mut endpoints = vec![format!("{}/models", base_url)];
    if root_url == base_url {
        endpoints.push(format!("{}/v1/models", base_url));
    } else {
        endpoints.push(format!("{}/models", root_url));
    }

    endpoints
}

fn build_ollama_tags_endpoints(base_url: &str) -> Vec<String> {
    let (base_url, root_url) = normalize_base_url(base_url);

    let mut endpoints = vec![format!("{}/api/tags", base_url)];
    if root_url != base_url {
        endpoints.push(format!("{}/api/tags", root_url));
    }

    endpoints
}

fn parse_openai_models_count(json: &serde_json::Value) -> Option<u32> {
    json.get("data")
        .and_then(|d| d.as_array())
        .map(|arr| arr.len() as u32)
}

fn parse_openai_models(json: &serde_json::Value) -> Vec<crate::types::OpenAICompatibleModel> {
    json.get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m.get("id")?.as_str()?.to_string();
                    Some(crate::types::OpenAICompatibleModel {
                        id,
                        owned_by: m.get("owned_by").and_then(|v| v.as_str()).map(String::from),
                        created: m.get("created").and_then(|v| v.as_i64()),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn send_test_request(
    client: &Client,
    endpoint: &str,
    api_key: &str,
    payload: serde_json::Value,
) -> Result<Response, reqwest::Error> {
    client
        .post(endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&payload)
        .send()
        .await
}

fn format_transport_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "Connection timed out while testing the model".to_string()
    } else if error.is_connect() {
        "Could not connect to the local proxy while testing the model".to_string()
    } else {
        format!("Connection failed: {}", error)
    }
}

fn is_copilot_model_id(model_id: &str) -> bool {
    let id = model_id.to_lowercase();
    id.starts_with("github-copilot/")
        || id.starts_with("copilot-")
        || matches!(
            id.as_str(),
            "gpt-4.1"
                | "gpt-5"
                | "gpt-5-mini"
                | "gpt-5-codex"
                | "gpt-5.1"
                | "gpt-5.1-codex"
                | "gpt-5.1-codex-mini"
                | "gpt-4o"
                | "gpt-4"
                | "gpt-4-turbo"
                | "o1"
                | "o1-mini"
                | "grok-code-fast-1"
                | "raptor-mini"
                | "gemini-2.5-pro"
                | "gemini-3-pro-preview"
                | "gemini-3.1-pro-high"
                | "gemini-3.1-pro-low"
                | "claude-haiku-4.5"
                | "claude-opus-4.1"
                | "claude-sonnet-4"
                | "claude-sonnet-4.5"
                | "claude-opus-4.5"
                | "claude-opus-4.6"
        )
}

#[tauri::command]
pub fn get_gpt_reasoning_models() -> Vec<String> {
    crate::GPT5_BASE_MODELS.iter().map(|s| s.to_string()).collect()
}

#[tauri::command]
pub async fn get_available_models(state: State<'_, AppState>) -> Result<Vec<AvailableModel>, String> {
    let config = state.config.lock().unwrap().clone();
    let proxy_running = state.proxy_status.lock().unwrap().running;
    
    if !proxy_running {
        return Ok(vec![]);
    }
    
    // Get auth status to determine model sources
    let auth_status = state.auth_status.lock().unwrap().clone();
    let has_vertex = auth_status.vertex > 0;
    let has_gemini_api = !config.gemini_api_keys.is_empty();
    let has_gemini_web = auth_status.gemini_web > 0;
    let has_copilot = config.copilot.enabled;
    
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    
    let endpoint = format!("http://localhost:{}/v1/models", config.port);
    
    let response = match client.get(&endpoint)
        .header("Authorization", format!("Bearer {}", config.proxy_api_key))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            return Err(format!("Proxy not responding. Please restart the proxy. ({})", e));
        }
    };
    
    if !response.status().is_success() {
        return Err(format!("API returned status {}", response.status()));
    }
    
    let api_response: ModelsApiResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse models response: {}", e))?;
    
    let models: Vec<AvailableModel> = api_response.data
        .into_iter()
        .map(|m| {
            // Determine source based on route/auth status
            let source = if m.owned_by == "copilot" || (has_copilot && is_copilot_model_id(&m.id)) {
                "copilot".to_string()
            } else {
                match m.owned_by.as_str() {
                    "google" => {
                        // Google models can come from Vertex AI, Gemini API, or Gemini Web
                        if has_vertex && has_gemini_api && has_gemini_web {
                            "vertex+gemini-api+gemini-web".to_string()
                        } else if has_vertex && has_gemini_api {
                            "vertex+gemini-api".to_string()
                        } else if has_vertex && has_gemini_web {
                            "vertex+gemini-web".to_string()
                        } else if has_gemini_api && has_gemini_web {
                            "gemini-api+gemini-web".to_string()
                        } else if has_vertex {
                            "vertex".to_string()
                        } else if has_gemini_api {
                            "gemini-api".to_string()
                        } else if has_gemini_web {
                            "gemini-web".to_string()
                        } else {
                            "google".to_string() // Fallback
                        }
                    }
                    "anthropic" => {
                        if !config.claude_api_keys.is_empty() {
                            "api-key".to_string()
                        } else {
                            "oauth".to_string()
                        }
                    }
                    "openai" => {
                        if !config.codex_api_keys.is_empty() {
                            "api-key".to_string()
                        } else {
                            "oauth".to_string()
                        }
                    }
                    "ollama" => "ollama".to_string(),
                    owner => owner.to_string(),
                }
            };
            
            AvailableModel {
                id: m.id,
                owned_by: m.owned_by,
                source,
            }
        })
        .collect();
    
    Ok(models)
}

#[tauri::command]
pub async fn test_provider_connection(
    model_id: String,
    state: State<'_, AppState>,
) -> Result<ProviderTestResult, String> {
    let (port, api_key) = {
        let config = state.config.lock().unwrap();
        (config.port, config.proxy_api_key.clone())
    };

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let chat_endpoint = format!("http://localhost:{}/v1/chat/completions", port);
    let completions_endpoint = format!("http://localhost:{}/v1/completions", port);
    let chat_payload = serde_json::json!({
        "model": model_id,
        "messages": [
            {
                "role": "user",
                "content": "Say 'OK'"
            }
        ],
        "max_tokens": 5
    });
    let completions_payload = serde_json::json!({
        "model": model_id,
        "prompt": "Say 'OK'",
        "max_tokens": 5
    });

    let start = std::time::Instant::now();
    let response = send_test_request(&client, &chat_endpoint, &api_key, chat_payload).await;

    let final_result = match response {
        Ok(resp) if resp.status().is_success() => ProviderTestResult {
            success: true,
            message: "Connection successful!".to_string(),
            latency_ms: Some(start.elapsed().as_millis() as u64),
            models_found: None,
        },
        Ok(resp) => {
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_default();

            if is_completion_fallback_candidate(status, &error_text) {
                match send_test_request(&client, &completions_endpoint, &api_key, completions_payload).await {
                    Ok(fallback_resp) if fallback_resp.status().is_success() => ProviderTestResult {
                        success: true,
                        message: "Connection successful via completions fallback.".to_string(),
                        latency_ms: Some(start.elapsed().as_millis() as u64),
                        models_found: None,
                    },
                    Ok(fallback_resp) => {
                        let fallback_status = fallback_resp.status();
                        let fallback_text = fallback_resp.text().await.unwrap_or_default();
                        ProviderTestResult {
                            success: false,
                            message: format_openai_error(fallback_status, &fallback_text),
                            latency_ms: Some(start.elapsed().as_millis() as u64),
                            models_found: None,
                        }
                    }
                    Err(error) => ProviderTestResult {
                        success: false,
                        message: format_transport_error(&error),
                        latency_ms: Some(start.elapsed().as_millis() as u64),
                        models_found: None,
                    },
                }
            } else {
                ProviderTestResult {
                    success: false,
                    message: format_openai_error(status, &error_text),
                    latency_ms: Some(start.elapsed().as_millis() as u64),
                    models_found: None,
                }
            }
        }
        Err(error) => ProviderTestResult {
            success: false,
            message: format_transport_error(&error),
            latency_ms: Some(start.elapsed().as_millis() as u64),
            models_found: None,
        },
    };

    Ok(final_result)
}

#[tauri::command]
pub async fn test_openai_provider(
    base_url: String,
    api_key: String,
    headers: Option<std::collections::HashMap<String, String>>,
) -> Result<ProviderTestResult, String> {
    if base_url.is_empty() {
        return Ok(ProviderTestResult {
            success: false,
            message: "Base URL is required".to_string(),
            latency_ms: None,
            models_found: None,
        });
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let endpoints = build_openai_model_endpoints(&base_url);

    let start = std::time::Instant::now();

    for endpoint in &endpoints {
        let mut request = client.get(endpoint);
        if !api_key.is_empty() {
            request = request.header("Authorization", format!("Bearer {}", api_key));
        }
        if let Some(headers) = &headers {
            for (key, value) in headers {
                request = request.header(key, value);
            }
        }
        let response = request.send().await;
        let latency = start.elapsed().as_millis() as u64;

        match response {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    let models_count = if let Ok(json) = resp.json::<serde_json::Value>().await {
                        parse_openai_models_count(&json)
                    } else {
                        None
                    };

                    return Ok(ProviderTestResult {
                        success: true,
                        message: format!("Connection successful! ({}ms)", latency),
                        latency_ms: Some(latency),
                        models_found: models_count,
                    });
                } else if status.as_u16() == 401 || status.as_u16() == 403 {
                    return Ok(ProviderTestResult {
                        success: false,
                        message: "Authentication failed - check your API key or headers".to_string(),
                        latency_ms: Some(latency),
                        models_found: None,
                    });
                }
                // For 404, try the next endpoint pattern
            }
            Err(e) => {
                // For connection errors, return immediately
                if e.is_timeout() {
                    return Ok(ProviderTestResult {
                        success: false,
                        message: "Connection timed out - check your base URL".to_string(),
                        latency_ms: Some(start.elapsed().as_millis() as u64),
                        models_found: None,
                    });
                } else if e.is_connect() {
                    return Ok(ProviderTestResult {
                        success: false,
                        message: "Could not connect - check your base URL".to_string(),
                        latency_ms: Some(start.elapsed().as_millis() as u64),
                        models_found: None,
                    });
                }
            }
        }
    }

    // All endpoints failed with 404 or similar
    let latency = start.elapsed().as_millis() as u64;
    Ok(ProviderTestResult {
        success: false,
        message: "Provider returned 404 Not Found - check your base URL (tried /models and /v1/models)".to_string(),
        latency_ms: Some(latency),
        models_found: None,
    })
}

#[tauri::command]
pub async fn test_ollama_provider(
    base_url: String,
    api_key: String,
    headers: Option<std::collections::HashMap<String, String>>,
) -> Result<ProviderTestResult, String> {
    if base_url.is_empty() {
        return Ok(ProviderTestResult {
            success: false,
            message: "Base URL is required".to_string(),
            latency_ms: None,
            models_found: None,
        });
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let start = std::time::Instant::now();

    for endpoint in build_ollama_tags_endpoints(&base_url) {
        let mut request = client.get(&endpoint);
        if !api_key.is_empty() {
            request = request.header("Authorization", format!("Bearer {}", api_key));
        }
        if let Some(headers) = &headers {
            for (key, value) in headers {
                request = request.header(key, value);
            }
        }

        match request.send().await {
            Ok(resp) if resp.status().is_success() => {
                let latency = start.elapsed().as_millis() as u64;
                let models_count = resp
                    .json::<OllamaTagsResponse>()
                    .await
                    .ok()
                    .map(|json| json.models.len() as u32);
                return Ok(ProviderTestResult {
                    success: true,
                    message: format!("Connection successful! ({}ms)", latency),
                    latency_ms: Some(latency),
                    models_found: models_count,
                });
            }
            Ok(resp) if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 => {
                let latency = start.elapsed().as_millis() as u64;
                return Ok(ProviderTestResult {
                    success: false,
                    message: "Authentication failed - check your API key or headers".to_string(),
                    latency_ms: Some(latency),
                    models_found: None,
                });
            }
            Err(e) if e.is_timeout() => {
                return Ok(ProviderTestResult {
                    success: false,
                    message: "Connection timed out - check your base URL".to_string(),
                    latency_ms: Some(start.elapsed().as_millis() as u64),
                    models_found: None,
                });
            }
            Err(e) if e.is_connect() => {
                return Ok(ProviderTestResult {
                    success: false,
                    message: "Could not connect - check your base URL".to_string(),
                    latency_ms: Some(start.elapsed().as_millis() as u64),
                    models_found: None,
                });
            }
            _ => continue,
        }
    }

    for endpoint in build_openai_model_endpoints(&base_url) {
        let mut request = client.get(&endpoint);
        if !api_key.is_empty() {
            request = request.header("Authorization", format!("Bearer {}", api_key));
        }
        if let Some(headers) = &headers {
            for (key, value) in headers {
                request = request.header(key, value);
            }
        }

        match request.send().await {
            Ok(resp) if resp.status().is_success() => {
                let latency = start.elapsed().as_millis() as u64;
                let models_count = resp
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .and_then(|json| parse_openai_models_count(&json));
                return Ok(ProviderTestResult {
                    success: true,
                    message: format!("Connection successful! ({}ms)", latency),
                    latency_ms: Some(latency),
                    models_found: models_count,
                });
            }
            Ok(resp) if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 => {
                let latency = start.elapsed().as_millis() as u64;
                return Ok(ProviderTestResult {
                    success: false,
                    message: "Authentication failed - check your API key or headers".to_string(),
                    latency_ms: Some(latency),
                    models_found: None,
                });
            }
            Err(e) if e.is_timeout() => {
                return Ok(ProviderTestResult {
                    success: false,
                    message: "Connection timed out - check your base URL".to_string(),
                    latency_ms: Some(start.elapsed().as_millis() as u64),
                    models_found: None,
                });
            }
            Err(e) if e.is_connect() => {
                return Ok(ProviderTestResult {
                    success: false,
                    message: "Could not connect - check your base URL".to_string(),
                    latency_ms: Some(start.elapsed().as_millis() as u64),
                    models_found: None,
                });
            }
            _ => continue,
        }
    }

    let latency = start.elapsed().as_millis() as u64;
    Ok(ProviderTestResult {
        success: false,
        message: "Provider returned 404 Not Found - check your base URL (tried /api/tags, /models, and /v1/models)".to_string(),
        latency_ms: Some(latency),
        models_found: None,
    })
}

// Fetch models from all configured OpenAI-compatible providers
#[tauri::command]
pub async fn fetch_openai_compatible_models(
    state: State<'_, AppState>,
    providers: Option<Vec<crate::types::OpenAICompatibleProvider>>,
) -> Result<Vec<crate::types::OpenAICompatibleProviderModels>, String> {
    // Get all configured OpenAI-compatible providers unless a specific list was provided
    let providers = match providers {
        Some(providers) => providers,
        None => crate::commands::api_keys::get_openai_compatible_providers(state.clone()).await?,
    };
    
    if providers.is_empty() {
        return Ok(Vec::new());
    }
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();

    for provider in providers {
        let base_url = provider.base_url.trim_end_matches('/');
        let api_key = provider.api_key_entries.first()
            .map(|e| e.api_key.clone())
            .unwrap_or_default();
        let headers = provider.headers.clone().unwrap_or_default();

        // Try OpenAI-compatible endpoint patterns only
        let endpoints = vec![(format!("{}/models", base_url), false), (format!("{}/v1/models", base_url), false)];

        let mut found_models = false;

        for (endpoint, is_ollama_tags) in &endpoints {
            let mut request = client.get(endpoint);
            if !api_key.is_empty() {
                request = request.header("Authorization", format!("Bearer {}", api_key));
            }
            for (key, value) in &headers {
                request = request.header(key, value);
            }
            let response = request.send().await;

            match response {
                Ok(resp) if resp.status().is_success() => {
                    if *is_ollama_tags {
                        if let Ok(json) = resp.json::<OllamaTagsResponse>().await {
                            let models = json
                                .models
                                .into_iter()
                                .map(|m| crate::types::OpenAICompatibleModel {
                                    id: m.name,
                                    owned_by: Some("ollama".to_string()),
                                    created: None,
                                })
                                .collect();

                            results.push(crate::types::OpenAICompatibleProviderModels {
                                provider_name: provider.name.clone(),
                                base_url: provider.base_url.clone(),
                                models,
                                error: None,
                            });
                            found_models = true;
                            break;
                        }
                    } else if let Ok(json) = resp.json::<serde_json::Value>().await {
                        let models: Vec<crate::types::OpenAICompatibleModel> = json
                            .get("data")
                            .and_then(|d| d.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|m| {
                                        let id = m.get("id")?.as_str()?.to_string();
                                        Some(crate::types::OpenAICompatibleModel {
                                            id,
                                            owned_by: m.get("owned_by").and_then(|v| v.as_str()).map(String::from),
                                            created: m.get("created").and_then(|v| v.as_i64()),
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();

                        results.push(crate::types::OpenAICompatibleProviderModels {
                            provider_name: provider.name.clone(),
                            base_url: provider.base_url.clone(),
                            models,
                            error: None,
                        });
                        found_models = true;
                        break;
                    }
                }
                Ok(resp) if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 => {
                    results.push(crate::types::OpenAICompatibleProviderModels {
                        provider_name: provider.name.clone(),
                        base_url: provider.base_url.clone(),
                        models: Vec::new(),
                        error: Some("Authentication failed".to_string()),
                    });
                    found_models = true;
                    break;
                }
                _ => continue, // Try next endpoint
            }
        }

        if !found_models {
            results.push(crate::types::OpenAICompatibleProviderModels {
                provider_name: provider.name.clone(),
                base_url: provider.base_url.clone(),
                models: Vec::new(),
                error: Some("Could not fetch models - endpoint not found".to_string()),
            });
        }
    }

    Ok(results)
}

#[tauri::command]
pub async fn fetch_ollama_models(
    state: State<'_, AppState>,
    providers: Option<Vec<crate::types::OpenAICompatibleProvider>>,
) -> Result<Vec<crate::types::OpenAICompatibleProviderModels>, String> {
    let providers = match providers {
        Some(providers) => providers,
        None => crate::commands::api_keys::get_ollama_providers(state.clone()).await?,
    };

    if providers.is_empty() {
        return Ok(Vec::new());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();

    for provider in providers {
        let api_key = provider.api_key_entries.first()
            .map(|e| e.api_key.clone())
            .unwrap_or_default();
        let headers = provider.headers.clone().unwrap_or_default();
        let mut found_models = false;

        for endpoint in build_ollama_tags_endpoints(&provider.base_url) {
            let mut request = client.get(&endpoint);
            if !api_key.is_empty() {
                request = request.header("Authorization", format!("Bearer {}", api_key));
            }
            for (key, value) in &headers {
                request = request.header(key, value);
            }

            match request.send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(json) = resp.json::<OllamaTagsResponse>().await {
                        let models = json.models.into_iter().map(|m| crate::types::OpenAICompatibleModel {
                            id: m.name,
                            owned_by: Some("ollama".to_string()),
                            created: None,
                        }).collect();
                        results.push(crate::types::OpenAICompatibleProviderModels {
                            provider_name: provider.name.clone(),
                            base_url: provider.base_url.clone(),
                            models,
                            error: None,
                        });
                        found_models = true;
                        break;
                    }
                }
                Ok(resp) if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 => {
                    results.push(crate::types::OpenAICompatibleProviderModels {
                        provider_name: provider.name.clone(),
                        base_url: provider.base_url.clone(),
                        models: Vec::new(),
                        error: Some("Authentication failed".to_string()),
                    });
                    found_models = true;
                    break;
                }
                _ => continue,
            }
        }

        if found_models {
            continue;
        }

        for endpoint in build_openai_model_endpoints(&provider.base_url) {
            let mut request = client.get(&endpoint);
            if !api_key.is_empty() {
                request = request.header("Authorization", format!("Bearer {}", api_key));
            }
            for (key, value) in &headers {
                request = request.header(key, value);
            }

            match request.send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        results.push(crate::types::OpenAICompatibleProviderModels {
                            provider_name: provider.name.clone(),
                            base_url: provider.base_url.clone(),
                            models: parse_openai_models(&json),
                            error: None,
                        });
                        found_models = true;
                        break;
                    }
                }
                Ok(resp) if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 => {
                    results.push(crate::types::OpenAICompatibleProviderModels {
                        provider_name: provider.name.clone(),
                        base_url: provider.base_url.clone(),
                        models: Vec::new(),
                        error: Some("Authentication failed".to_string()),
                    });
                    found_models = true;
                    break;
                }
                _ => continue,
            }
        }

        if !found_models {
            results.push(crate::types::OpenAICompatibleProviderModels {
                provider_name: provider.name.clone(),
                base_url: provider.base_url.clone(),
                models: Vec::new(),
                error: Some("Could not fetch models - endpoint not found".to_string()),
            });
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::build_ollama_tags_endpoints;

    #[test]
    fn build_ollama_tags_endpoints_uses_api_tags_for_plain_base_url() {
        assert_eq!(
            build_ollama_tags_endpoints("http://127.0.0.1:11434"),
            vec!["http://127.0.0.1:11434/api/tags".to_string()]
        );
    }

    #[test]
    fn build_ollama_tags_endpoints_maps_v1_base_url_to_root_api_tags() {
        assert_eq!(
            build_ollama_tags_endpoints("http://127.0.0.1:8317/v1"),
            vec![
                "http://127.0.0.1:8317/v1/api/tags".to_string(),
                "http://127.0.0.1:8317/api/tags".to_string()
            ]
        );
    }

    #[test]
    fn build_ollama_tags_endpoints_trims_trailing_slash() {
        assert_eq!(
            build_ollama_tags_endpoints("http://127.0.0.1:11434/"),
            vec!["http://127.0.0.1:11434/api/tags".to_string()]
        );
    }

    #[test]
    fn build_openai_model_endpoints_uses_v1_for_root_shared_endpoint() {
        assert_eq!(
            super::build_openai_model_endpoints("http://127.0.0.1:8317"),
            vec![
                "http://127.0.0.1:8317/models".to_string(),
                "http://127.0.0.1:8317/v1/models".to_string()
            ]
        );
    }

    #[test]
    fn build_openai_model_endpoints_uses_root_models_for_v1_shared_endpoint() {
        assert_eq!(
            super::build_openai_model_endpoints("http://127.0.0.1:8317/v1"),
            vec![
                "http://127.0.0.1:8317/v1/models".to_string(),
                "http://127.0.0.1:8317/models".to_string()
            ]
        );
    }
}

// Get model context and output limits
pub(crate) fn get_model_limits(model_id: &str, owned_by: &str, source: &str) -> (u64, u64) {
    // Return (context_limit, output_limit)
    // First check model_id patterns (handles Antigravity Claude models like claude-opus-4-5-thinking)
    let model_lower = model_id.to_lowercase();
    
    // Claude models (direct or via Antigravity)
    if model_lower.contains("claude") {
        // Claude 4.5 models: 200K context, 64K output
        // Claude 3.5 haiku: 200K context, 8K output
        if model_lower.contains("3-5-haiku") || model_lower.contains("3-haiku") {
            return (200000, 8192);
        } else {
            // sonnet-4-5, opus-4-5, haiku-4-5, and other Claude 4.x models
            return (200000, 64000);
        }
    }
    
    // Gemini models
    if model_lower.contains("gemini") {
        // Gemini 2.5 models: 1M context, 65K output
        return (1000000, 65536);
    }
    
    // GPT/OpenAI models
    if model_lower.contains("gpt") || model_lower.starts_with("o1") || model_lower.starts_with("o3") {
        // o1, o3 reasoning models: 200K context, 100K output
        if model_lower.contains("o3") || model_lower.contains("o1") {
            return (200000, 100000);
        } else if model_lower.contains("gpt-5") || model_lower.contains("gpt5") {
            // GPT-5 via Copilot: 128K context (Copilot limit)
            // GPT-5 via ChatGPT/ProxyPal: 400K context
            if source == "copilot" {
                return (128000, 32768);
            } else {
                return (400000, 32768);
            }
        } else {
            // gpt-4o, gpt-4o-mini, gpt-4.1: 128K context, 16K output
            return (128000, 16384);
        }
    }
    
    // Qwen models
    if model_lower.contains("qwen") {
        // Qwen3 Coder Plus: 1M context
        if model_lower.contains("coder") {
            return (1000000, 65536);
        } else {
            // Qwen3 models: 262K context (max), 65K output
            return (262144, 65536);
        }
    }
    
    // DeepSeek models
    if model_lower.contains("deepseek") {
        // deepseek-reasoner: 128K output, deepseek-chat: 8K output
        if model_lower.contains("reasoner") || model_lower.contains("r1") {
            return (128000, 128000);
        } else {
            return (128000, 8192);
        }
    }
    
    // Fallback to owned_by for any remaining models
    match owned_by {
        "anthropic" => (200000, 64000),
        "google" => (1000000, 65536),
        "openai" => (128000, 16384),
        "qwen" => (262144, 65536),
        "deepseek" => (128000, 8192),
        _ => (128000, 16384) // safe defaults
    }
}

// Get display name for a model
pub(crate) fn get_model_display_name(model_id: &str, owned_by: &str, source: &str) -> String {
    // Convert model ID to human-readable name
    let base_name = model_id
        .replace("-", " ")
        .replace(".", " ")
        .split_whitespace()
        .map(|word| {
            let mut chars: Vec<char> = word.chars().collect();
            if !chars.is_empty() {
                chars[0] = chars[0].to_uppercase().next().unwrap_or(chars[0]);
            }
            chars.into_iter().collect::<String>()
        })
        .collect::<Vec<String>>()
        .join(" ");
    
    // Add provider prefix for clarity
    let name = match owned_by {
        "copilot" => format!("Copilot {}", base_name),
        "anthropic" => format!("{}", base_name),
        "google" => format!("{}", base_name),
        "openai" => format!("{}", base_name),
        "qwen" => format!("{}", base_name),
        _ => base_name
    };
    
    // Add source indicator for Vertex AI and other special sources
    match source {
        "vertex" => format!("{} [Vertex]", name),
        "vertex+gemini-api" => format!("{} [Vertex+API]", name),
        "copilot" => format!("{} [Copilot]", name),
        _ => name
    }
}

#[tauri::command]
pub async fn set_claude_code_model(model_type: String, model_name: String) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("Could not find home directory")?;
    let config_dir = home.join(".claude");
    std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
    let config_path = config_dir.join("settings.json");
    
    // Read existing config or create new
    let mut json: serde_json::Value = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    
    // Ensure env object exists
    if json.get("env").is_none() {
        json["env"] = serde_json::json!({});
    }
    
    // Map model_type to env var name
    let env_key = match model_type.as_str() {
        "haiku" => "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "opus" => "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "sonnet" => "ANTHROPIC_DEFAULT_SONNET_MODEL",
        _ => return Err(format!("Unknown model type: {}", model_type)),
    };
    
    // Update the model
    if let Some(env) = json.get_mut("env").and_then(|e| e.as_object_mut()) {
        env.insert(env_key.to_string(), serde_json::Value::String(model_name));
    }
    
    // Write back
    let config_str = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, config_str).map_err(|e| e.to_string())?;
    
    Ok(())
}

// Get force model mappings from Management API
#[tauri::command]
pub async fn get_force_model_mappings(state: State<'_, AppState>) -> Result<bool, String> {
    let port = state.config.lock().unwrap().port;
    let url = crate::get_management_url(port, "ampcode/force-model-mappings");
    
    let client = crate::build_management_client();
    let response = client
        .get(&url)
        .header("X-Management-Key", &crate::get_management_key())
        .send()
        .await
        .map_err(|e| format!("Failed to get force model mappings: {}", e))?;
    
    if !response.status().is_success() {
        return Ok(false); // Default to false
    }
    
    let json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    Ok(json.get("force-model-mappings").and_then(|v| v.as_bool()).unwrap_or(false))
}

// Set force model mappings via Management API
#[tauri::command]
pub async fn set_force_model_mappings(state: State<'_, AppState>, value: bool) -> Result<(), String> {
    let port = state.config.lock().unwrap().port;
    let url = crate::get_management_url(port, "ampcode/force-model-mappings");
    
    let client = crate::build_management_client();
    let response = client
        .put(&url)
        .header("X-Management-Key", &crate::get_management_key())
        .json(&serde_json::json!({ "value": value }))
        .send()
        .await
        .map_err(|e| format!("Failed to set force model mappings: {}", e))?;
    
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Failed to set force model mappings: {} - {}", status, text));
    }
    
    // Persist to Tauri config so it survives restart
    let mut config = state.config.lock().unwrap();
    config.force_model_mappings = value;
    save_config_to_file(&config).map_err(|e| format!("Failed to save config: {}", e))?;
    
    Ok(())
}
