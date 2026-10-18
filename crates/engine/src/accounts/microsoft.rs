//! The Microsoft/Xbox/Minecraft HTTP steps. Both sign-in methods converge on the
//! same signed tail: Xbox device token → sisu authorize → XSTS → launcher/login
//! → profile.

use anyhow::{anyhow, bail, Context, Result};
use proto::error::Service;
use reqwest::header::HeaderMap;
use serde_json::{json, Value};

use super::signing::{xbox_signature_header, ProofKey};

const CLIENT_ID: &str = "00000000402b5328";
const REPLY_URL: &str = "https://login.live.com/oauth20_desktop.srf";
const SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";
const TITLE_ID: &str = "1794566092";
pub(crate) const USER_AGENT: &str = "Hestia/1.0 (+https://github.com/prytaneum/hestia)";

const DEVICE_AUTH_URL: &str = "https://device.auth.xboxlive.com/device/authenticate";
const SISU_AUTHENTICATE_URL: &str = "https://sisu.xboxlive.com/authenticate";
const DEVICE_CODE_URL: &str = "https://login.live.com/oauth20_connect.srf";
const OAUTH_TOKEN_URL: &str = "https://login.live.com/oauth20_token.srf";
const SISU_AUTHORIZE_URL: &str = "https://sisu.xboxlive.com/authorize";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const LAUNCHER_LOGIN_URL: &str = "https://api.minecraftservices.com/launcher/login";
const PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

pub struct DeviceToken {
    pub token: String,
    pub clock_offset: i64,
}

pub struct SisuAuthentication {
    pub session_id: String,
    pub url: String,
}

pub struct DeviceCodeChallenge {
    pub user_code: String,
    pub verification_uri: String,
    pub device_code: String,
    pub interval_seconds: i64,
    pub expires_in_seconds: i64,
}

#[derive(Default)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
}

/// The refresh token is dead (`invalid_grant`) — as opposed to a transient
/// error; callers treat only this as re-auth-required.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct OAuthRejected(pub String);

pub struct SisuAuthorization {
    pub user_token: String,
    pub title_token: String,
}

pub struct XstsToken {
    pub token: String,
    pub user_hash: String,
}

pub struct MinecraftProfile {
    pub uuid: String,
    pub name: String,
}

/// Every step goes through the shared client, so a sign-in started while the
/// network is dropping fails as offline instead of hanging on a dead socket.
async fn send(service: Service, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
    crate::net::send(Some(service), request).await
}

struct Parsed {
    headers: HeaderMap,
    body: Value,
}

async fn read(response: reqwest::Response, what: &str) -> Result<Parsed> {
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let text = response
        .text()
        .await
        .with_context(|| format!("{what} failed"))?;
    tracing::debug!(what, status, bytes = text.len(), "response");
    let body: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => {
            let shape = if text.is_empty() {
                "empty body"
            } else {
                "non-JSON body"
            };
            let mut message = format!("{what}: HTTP {status} ({shape})");
            if status == 403 {
                message.push_str(
                    "; this usually means the system clock is wrong — check the date and time settings",
                );
            }
            bail!(message);
        }
    };
    Ok(Parsed { headers, body })
}

fn require_string(body: &Value, key: &str, what: &str) -> Result<String> {
    match body.get(key).and_then(Value::as_str) {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        _ => Err(anyhow!("{what} response is missing {key}")),
    }
}

fn nested_token(body: &Value, key: &str, what: &str) -> Result<String> {
    let node = body
        .get(key)
        .filter(|v| v.is_object())
        .ok_or_else(|| anyhow!("{what} response is missing {key}"))?;
    require_string(node, "Token", what)
}

fn proof_jwk(key: &ProofKey) -> Value {
    json!({ "kty": "EC", "x": key.x(), "y": key.y(), "crv": "P-256", "alg": "ES256", "use": "sig" })
}

fn days_from_civil(mut year: i64, month: u32, day: u32) -> i64 {
    year -= (month <= 2) as i64;
    let era = (if year >= 0 { year } else { year - 399 }) / 400;
    let yoe = year - era * 400;
    let doy =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) as i64 + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

// Parse an RFC 1123 HTTP date ("Wed, 21 Oct 2015 07:28:00 GMT") to unix seconds.
fn parse_http_date(value: &str) -> Option<i64> {
    let rest = value.split_once(", ").map(|(_, r)| r).unwrap_or(value);
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }
    let day: u32 = parts[0].parse().ok()?;
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = MONTHS.iter().position(|m| *m == parts[1])? as u32 + 1;
    let year: i64 = parts[2].parse().ok()?;
    let time: Vec<&str> = parts[3].split(':').collect();
    if time.len() != 3 {
        return None;
    }
    let hour: i64 = time[0].parse().ok()?;
    let minute: i64 = time[1].parse().ok()?;
    let second: i64 = time[2].parse().ok()?;
    Some(days_from_civil(year, month, day) * 86400 + hour * 3600 + minute * 60 + second)
}

fn server_clock_offset(headers: &HeaderMap) -> i64 {
    let Some(date) = headers.get("date").and_then(|v| v.to_str().ok()) else {
        return 0;
    };
    let Some(server_time) = parse_http_date(date) else {
        return 0;
    };
    let offset = server_time - common::now_unix();
    if offset.abs() > 60 {
        tracing::warn!(
            offset,
            "system clock differs from Xbox server time; correcting signatures"
        );
    }
    offset
}

async fn signed_post(
    url: &str,
    url_path: &str,
    body: &Value,
    key: &ProofKey,
    contract_version: bool,
    clock_offset: i64,
) -> Result<reqwest::Response> {
    let payload = body.to_string();
    let signature = xbox_signature_header(
        key,
        url_path,
        "",
        &payload,
        common::now_unix() + clock_offset,
    );
    let mut request = crate::net::client()
        .post(url)
        .header("Content-Type", "application/json; charset=utf-8")
        .header("Accept", "application/json")
        .header("Signature", signature);
    if contract_version {
        request = request.header("x-xbl-contract-version", "1");
    }
    send(Service::Xbox, request.body(payload)).await
}

pub async fn request_device_token(key: &ProofKey) -> Result<DeviceToken> {
    let body = json!({
        "Properties": {
            "AuthMethod": "ProofOfPossession",
            "Id": format!("{{{}}}", key.id().to_uppercase()),
            "DeviceType": "Win32",
            "Version": "10.16.0",
            "ProofKey": proof_jwk(key),
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT",
    });
    let response =
        signed_post(DEVICE_AUTH_URL, "/device/authenticate", &body, key, true, 0).await?;
    let parsed = read(response, "Xbox device token").await?;
    Ok(DeviceToken {
        token: require_string(&parsed.body, "Token", "Xbox device token")?,
        clock_offset: server_clock_offset(&parsed.headers),
    })
}

pub async fn sisu_authenticate(
    device_token: &str,
    challenge: &str,
    state: &str,
    key: &ProofKey,
    clock_offset: i64,
) -> Result<SisuAuthentication> {
    let body = json!({
        "AppId": CLIENT_ID,
        "DeviceToken": device_token,
        "Offers": [SCOPE],
        "Query": {
            "code_challenge": challenge,
            "code_challenge_method": "S256",
            "state": state,
            "prompt": "select_account",
        },
        "RedirectUri": REPLY_URL,
        "Sandbox": "RETAIL",
        "TokenType": "code",
        "TitleId": TITLE_ID,
    });
    let response = signed_post(
        SISU_AUTHENTICATE_URL,
        "/authenticate",
        &body,
        key,
        true,
        clock_offset,
    )
    .await?;
    let parsed = read(response, "Xbox sign-in request").await?;
    let session_id = parsed
        .headers
        .get("x-sessionid")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Xbox sign-in request did not return a session id"))?
        .to_string();
    Ok(SisuAuthentication {
        session_id,
        url: require_string(&parsed.body, "MsaOauthRedirect", "Xbox sign-in request")?,
    })
}

async fn exchange_oauth(form: &[(&str, &str)], what: &str, rejection: &str) -> Result<OAuthTokens> {
    let response = send(
        Service::Microsoft,
        crate::net::client()
            .post(OAUTH_TOKEN_URL)
            .header("Accept", "application/json")
            .form(form),
    )
    .await
    .with_context(|| format!("{what} failed"))?;
    let parsed = read(response, what).await?;
    if let Some(error) = parsed.body.get("error").and_then(Value::as_str) {
        let description = parsed
            .body
            .get("error_description")
            .and_then(Value::as_str)
            .unwrap_or(error);
        // Only `invalid_grant` means the refresh token itself is dead. Other
        // OAuth errors (server_error, temporarily_unavailable, …) are transient
        // and must not force a re-login.
        if error == "invalid_grant" {
            return Err(OAuthRejected(format!("{rejection}: {description}")).into());
        }
        bail!("{rejection}: {description}");
    }
    Ok(OAuthTokens {
        access_token: require_string(&parsed.body, "access_token", what)?,
        refresh_token: parsed
            .body
            .get("refresh_token")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

pub async fn request_device_code() -> Result<DeviceCodeChallenge> {
    let response = send(
        Service::Microsoft,
        crate::net::client()
            .post(DEVICE_CODE_URL)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", CLIENT_ID),
                ("scope", SCOPE),
                ("response_type", "device_code"),
            ]),
    )
    .await
    .context("device sign-in request failed")?;
    let parsed = read(response, "device sign-in request").await?;
    if let Some(error) = parsed.body.get("error").and_then(Value::as_str) {
        let description = parsed
            .body
            .get("error_description")
            .and_then(Value::as_str)
            .unwrap_or(error);
        bail!("Microsoft declined the device sign-in request: {description}");
    }
    Ok(DeviceCodeChallenge {
        user_code: require_string(&parsed.body, "user_code", "device sign-in request")?,
        verification_uri: require_string(
            &parsed.body,
            "verification_uri",
            "device sign-in request",
        )?,
        device_code: require_string(&parsed.body, "device_code", "device sign-in request")?,
        interval_seconds: parsed
            .body
            .get("interval")
            .and_then(Value::as_i64)
            .unwrap_or(5),
        expires_in_seconds: parsed
            .body
            .get("expires_in")
            .and_then(Value::as_i64)
            .unwrap_or(900),
    })
}

/// Poll the device-code grant. `Ok(None)` means "keep waiting"
/// (authorization_pending / slow_down).
pub async fn poll_device_code(device_code: &str) -> Result<Option<OAuthTokens>> {
    let response = send(
        Service::Microsoft,
        crate::net::client()
            .post(OAUTH_TOKEN_URL)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", CLIENT_ID),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", device_code),
            ]),
    )
    .await
    .context("device sign-in poll failed")?;
    let parsed = read(response, "device sign-in poll").await?;
    let error = parsed
        .body
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if error.is_empty() {
        return Ok(Some(OAuthTokens {
            access_token: require_string(&parsed.body, "access_token", "device sign-in poll")?,
            refresh_token: parsed
                .body
                .get("refresh_token")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }));
    }
    match error {
        "authorization_pending" | "slow_down" => Ok(None),
        "authorization_declined" => bail!(proto::error::ErrorInfo::LoginDeclined),
        "expired_token" => bail!(proto::error::ErrorInfo::LoginTimedOut),
        _ => {
            let description = parsed
                .body
                .get("error_description")
                .and_then(Value::as_str)
                .unwrap_or(error);
            bail!("Microsoft rejected the sign-in: {description}");
        }
    }
}

pub async fn redeem_code(code: &str, verifier: &str) -> Result<OAuthTokens> {
    exchange_oauth(
        &[
            ("client_id", CLIENT_ID),
            ("code", code),
            ("code_verifier", verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", REPLY_URL),
            ("scope", SCOPE),
        ],
        "Microsoft token exchange",
        "Microsoft rejected the sign-in code",
    )
    .await
}

pub async fn refresh_oauth(refresh_token: &str) -> Result<OAuthTokens> {
    exchange_oauth(
        &[
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
            ("redirect_uri", REPLY_URL),
            ("scope", SCOPE),
        ],
        "Microsoft token refresh",
        "Microsoft rejected the token refresh",
    )
    .await
}

pub async fn sisu_authorize(
    session_id: &str,
    access_token: &str,
    device_token: &str,
    key: &ProofKey,
    clock_offset: i64,
) -> Result<SisuAuthorization> {
    let body = json!({
        "AccessToken": format!("t={access_token}"),
        "AppId": CLIENT_ID,
        "DeviceToken": device_token,
        "ProofKey": proof_jwk(key),
        "Sandbox": "RETAIL",
        "SessionId": if session_id.is_empty() { Value::Null } else { Value::String(session_id.to_string()) },
        "SiteName": "user.auth.xboxlive.com",
        "RelyingParty": "http://xboxlive.com",
        "UseModernGamertag": true,
    });
    let response = signed_post(
        SISU_AUTHORIZE_URL,
        "/authorize",
        &body,
        key,
        false,
        clock_offset,
    )
    .await?;
    let parsed = read(response, "Xbox authorization").await?;
    Ok(SisuAuthorization {
        user_token: nested_token(&parsed.body, "UserToken", "Xbox authorization")?,
        title_token: nested_token(&parsed.body, "TitleToken", "Xbox authorization")?,
    })
}

fn xsts_error_message(xerr: i64) -> String {
    match xerr {
        2148916233 => "this Microsoft account has no Xbox profile; sign in once at https://www.xbox.com and retry".into(),
        2148916235 => "Xbox Live is not available in this account's country or region".into(),
        2148916236 | 2148916237 => "this account needs adult verification on the Xbox homepage".into(),
        2148916238 => "this is a child account; an adult must add it to a Microsoft family first".into(),
        _ => format!("Xbox denied the sign-in (XErr {xerr})"),
    }
}

pub async fn xsts_authorize(
    authorization: &SisuAuthorization,
    device_token: &str,
    key: &ProofKey,
    clock_offset: i64,
) -> Result<XstsToken> {
    let body = json!({
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT",
        "Properties": {
            "SandboxId": "RETAIL",
            "UserTokens": [authorization.user_token],
            "DeviceToken": device_token,
            "TitleToken": authorization.title_token,
        },
    });
    let response = signed_post(XSTS_URL, "/xsts/authorize", &body, key, true, clock_offset).await?;
    if response.status().as_u16() == 401 {
        let text = response.text().await.unwrap_or_default();
        let xerr = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|d| d.get("XErr").and_then(Value::as_i64))
            .unwrap_or(0);
        bail!(xsts_error_message(xerr));
    }
    let parsed = read(response, "Xbox XSTS authorization").await?;
    let user_hash = parsed
        .body
        .get("DisplayClaims")
        .and_then(|c| c.get("xui"))
        .and_then(|x| x.get(0))
        .and_then(|c| c.get("uhs"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Xbox XSTS response is missing the user hash"))?
        .to_string();
    Ok(XstsToken {
        token: require_string(&parsed.body, "Token", "Xbox XSTS authorization")?,
        user_hash,
    })
}

pub struct MinecraftToken {
    pub access_token: String,
    pub expires_in: i64,
}

pub async fn launcher_login(xsts: &XstsToken) -> Result<MinecraftToken> {
    let body = json!({
        "platform": "PC_LAUNCHER",
        "xtoken": format!("XBL3.0 x={};{}", xsts.user_hash, xsts.token),
    });
    let response = send(
        Service::Mojang,
        crate::net::client()
            .post(LAUNCHER_LOGIN_URL)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT)
            .body(body.to_string()),
    )
    .await
    .context("Minecraft services sign-in failed")?;
    if response.status().as_u16() != 200 {
        bail!(
            "Minecraft services sign-in failed (HTTP {})",
            response.status().as_u16()
        );
    }
    let parsed = read(response, "Minecraft services").await?;
    Ok(MinecraftToken {
        access_token: require_string(&parsed.body, "access_token", "Minecraft services")?,
        expires_in: parsed
            .body
            .get("expires_in")
            .and_then(Value::as_i64)
            .unwrap_or(86_400),
    })
}

pub async fn minecraft_profile(minecraft_access_token: &str) -> Result<MinecraftProfile> {
    let response = send(
        Service::Mojang,
        crate::net::client()
            .get(PROFILE_URL)
            .header("Authorization", format!("Bearer {minecraft_access_token}"))
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT),
    )
    .await
    .context("Minecraft profile fetch failed")?;
    if response.status().as_u16() == 404 {
        bail!(
            "this Microsoft account owns no Minecraft profile; buy Minecraft: Java Edition or \
             create the profile in the official launcher first"
        );
    }
    let status = response.status().as_u16();
    let parsed = read(response, "Minecraft profile fetch").await?;
    if status != 200 {
        bail!("Minecraft profile fetch failed (HTTP {status})");
    }
    Ok(MinecraftProfile {
        uuid: require_string(&parsed.body, "id", "Minecraft profile")?,
        name: require_string(&parsed.body, "name", "Minecraft profile")?,
    })
}
