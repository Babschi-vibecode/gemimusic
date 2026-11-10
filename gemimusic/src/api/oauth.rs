use super::models::{AuthTokens, DeviceCodeResponse, UserProfile};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

// Public client credentials for Google OAuth (YouTube on TV / Smart TV / Android client)
// This client ID supports the OAuth 2.0 Device Flow for YouTube API
pub const DEFAULT_CLIENT_ID: &str = "861556708465-qhhnphik5nv8q3gm8lsn5749f7e8iu3d.apps.googleusercontent.com";

pub struct GoogleOAuthClient {
    client: Client,
    client_id: String,
}

impl Default for GoogleOAuthClient {
    fn default() -> Self {
        Self::new(DEFAULT_CLIENT_ID.to_string())
    }
}

impl GoogleOAuthClient {
    pub fn new(client_id: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            client_id,
        }
    }

    /// Step 1: Request Device Code and User Code from Google OAuth endpoint
    pub async fn request_device_code(&self) -> Result<DeviceCodeResponse, Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = "https://oauth2.googleapis.com/device/code";
        let params = [
            ("client_id", self.client_id.as_str()),
            (
                "scope",
                "https://www.googleapis.com/auth/youtube.readonly https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/userinfo.email",
            ),
        ];

        let res = self.client.post(endpoint).form(&params).send().await?;
        let json: Value = res.json().await?;

        let device_code = json.get("device_code").and_then(|v| v.as_str()).ok_or("Missing device_code")?.to_string();
        let user_code = json.get("user_code").and_then(|v| v.as_str()).ok_or("Missing user_code")?.to_string();
        let verification_url = json
            .get("verification_url")
            .and_then(|v| v.as_str())
            .unwrap_or("https://www.google.com/device")
            .to_string();
        let expires_in = json.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(1800);
        let interval = json.get("interval").and_then(|v| v.as_u64()).unwrap_or(5);

        Ok(DeviceCodeResponse {
            device_code,
            user_code,
            verification_url,
            expires_in,
            interval,
        })
    }

    /// Step 2: Poll Google token endpoint to check if user authorized the device code
    pub async fn poll_token(&self, device_code: &str) -> Result<Option<AuthTokens>, Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = "https://oauth2.googleapis.com/token";
        let params = [
            ("client_id", self.client_id.as_str()),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ];

        let res = self.client.post(endpoint).form(&params).send().await?;
        let json: Value = res.json().await?;

        if let Some(err) = json.get("error").and_then(|v| v.as_str()) {
            match err {
                "authorization_pending" => Ok(None), // Still waiting for user approval
                "slow_down" => Ok(None),             // Rate limited polling, wait longer
                "expired_token" => Err("Device authorization code expired. Please try again.".into()),
                "access_denied" => Err("User cancelled authorization request.".into()),
                other => Err(format!("OAuth error: {}", other).into()),
            }
        } else {
            let access_token = json.get("access_token").and_then(|v| v.as_str()).ok_or("Missing access_token")?.to_string();
            let refresh_token = json.get("refresh_token").and_then(|v| v.as_str()).map(|s| s.to_string());
            let token_type = json.get("token_type").and_then(|v| v.as_str()).unwrap_or("Bearer").to_string();
            let expires_in = json.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);

            let expires_at = chrono::Utc::now().timestamp() + expires_in;

            Ok(Some(AuthTokens {
                access_token,
                refresh_token,
                token_type,
                expires_at,
            }))
        }
    }

    /// Refresh expired access token using stored refresh token
    pub async fn refresh_access_token(&self, refresh_token: &str) -> Result<AuthTokens, Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = "https://oauth2.googleapis.com/token";
        let params = [
            ("client_id", self.client_id.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ];

        let res = self.client.post(endpoint).form(&params).send().await?;
        let json: Value = res.json().await?;

        let access_token = json.get("access_token").and_then(|v| v.as_str()).ok_or("Missing access_token in refresh response")?.to_string();
        let expires_in = json.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);
        let expires_at = chrono::Utc::now().timestamp() + expires_in;

        Ok(AuthTokens {
            access_token,
            refresh_token: Some(refresh_token.to_string()),
            token_type: "Bearer".to_string(),
            expires_at,
        })
    }

    /// Retrieve logged-in user profile (Name, Email, Picture)
    pub async fn get_user_profile(&self, access_token: &str) -> Result<UserProfile, Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = "https://www.googleapis.com/oauth2/v3/userinfo";
        let res = self.client.get(endpoint).bearer_auth(access_token).send().await?;
        let json: Value = res.json().await?;

        let email = json.get("email").and_then(|v| v.as_str()).unwrap_or("User").to_string();
        let name = json.get("name").and_then(|v| v.as_str()).unwrap_or("Google User").to_string();
        let picture = json.get("picture").and_then(|v| v.as_str()).unwrap_or("").to_string();

        Ok(UserProfile { email, name, picture })
    }
}
