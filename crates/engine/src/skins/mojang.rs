//! The Mojang profile-customization HTTP operations: read the profile's skins
//! and capes, change the skin (a PNG upload or a by-URL change), reset it, and
//! set or clear the active cape. All bearer-token calls against
//! `api.minecraftservices.com` — the token comes from the accounts subsystem.
//!
//! A skin change answers with the updated profile; it is parsed when possible
//! (it carries the texture key Mojang minted for the upload) and the caller
//! falls back to a fresh profile fetch when it cannot be read — the same shape
//! as Modrinth's implementation.

use anyhow::{bail, Context, Result};
use proto::error::Service;
use proto::skins::SkinVariant;
use serde_json::{json, Value};

use crate::accounts::USER_AGENT;

const PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const SKIN_URL: &str = "https://api.minecraftservices.com/minecraft/profile/skins";
const ACTIVE_SKIN_URL: &str = "https://api.minecraftservices.com/minecraft/profile/skins/active";
const ACTIVE_CAPE_URL: &str = "https://api.minecraftservices.com/minecraft/profile/capes/active";

#[derive(Debug, Clone)]
pub struct ProfileSkin {
    pub key: String,
    pub url: String,
    pub variant: SkinVariant,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct ProfileCape {
    pub id: String,
    pub name: String,
    pub url: String,
    pub active: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Profile {
    pub skins: Vec<ProfileSkin>,
    pub capes: Vec<ProfileCape>,
}

impl Profile {
    pub fn active_skin(&self) -> Option<&ProfileSkin> {
        self.skins.iter().find(|s| s.active)
    }
}

/// Every call goes through the shared client so a sign-in or skin change that
/// starts while the network is dropping fails as offline rather than hanging.
async fn send(service: Service, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
    crate::net::send(Some(service), request).await
}

pub async fn fetch_profile(token: &str) -> Result<Profile> {
    let response = send(
        Service::Mojang,
        crate::net::client()
            .get(PROFILE_URL)
            .bearer_auth(token)
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT),
    )
    .await?;
    let body = read(response, "Minecraft profile fetch").await?;
    Ok(parse_profile(&body))
}

/// Upload a skin PNG and make it active. Answers the updated profile when the
/// response carries one.
pub async fn upload_skin(
    token: &str,
    png: Vec<u8>,
    variant: SkinVariant,
) -> Result<Option<Profile>> {
    let form = reqwest::multipart::Form::new()
        .text("variant", variant_name(variant))
        .part(
            "file",
            reqwest::multipart::Part::bytes(png)
                .mime_str("image/png")
                .expect("static mime type parses")
                .file_name("skin.png"),
        );
    let response = send(
        Service::Mojang,
        crate::net::client()
            .post(SKIN_URL)
            .bearer_auth(token)
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT)
            .multipart(form),
    )
    .await?;
    let body = read(response, "Minecraft skin upload").await?;
    Ok(profile_when_present(&body))
}

/// Point the skin at a Mojang-hosted texture URL (how a vanilla default skin
/// is equipped without bundling its PNG).
pub async fn set_skin_url(token: &str, url: &str, variant: SkinVariant) -> Result<Option<Profile>> {
    let response = send(
        Service::Mojang,
        crate::net::client()
            .post(SKIN_URL)
            .bearer_auth(token)
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT)
            .json(&json!({ "variant": variant_name(variant), "url": url })),
    )
    .await?;
    let body = read(response, "Minecraft skin change").await?;
    Ok(profile_when_present(&body))
}

/// Reset the skin to the account's uuid-derived default.
pub async fn reset_skin(token: &str) -> Result<()> {
    let response = send(
        Service::Mojang,
        crate::net::client()
            .delete(ACTIVE_SKIN_URL)
            .bearer_auth(token)
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT),
    )
    .await?;
    read(response, "Minecraft skin reset").await?;
    Ok(())
}

pub async fn set_cape(token: &str, cape_id: &str) -> Result<()> {
    let response = send(
        Service::Mojang,
        crate::net::client()
            .put(ACTIVE_CAPE_URL)
            .bearer_auth(token)
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT)
            .json(&json!({ "capeId": cape_id })),
    )
    .await?;
    read(response, "Minecraft cape change").await?;
    Ok(())
}

pub async fn clear_cape(token: &str) -> Result<()> {
    let response = send(
        Service::Mojang,
        crate::net::client()
            .delete(ACTIVE_CAPE_URL)
            .bearer_auth(token)
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT),
    )
    .await?;
    read(response, "Minecraft cape clear").await?;
    Ok(())
}

/// Download a texture PNG (used to preserve the current skin before it is
/// replaced by a change).
pub async fn fetch_texture(url: &str) -> Result<Vec<u8>> {
    let response = send(
        Service::Mojang,
        crate::net::client()
            .get(url)
            .header("User-Agent", USER_AGENT),
    )
    .await?;
    if !response.status().is_success() {
        bail!(
            "skin texture fetch from {url} failed (HTTP {})",
            response.status().as_u16()
        );
    }
    Ok(response.bytes().await?.to_vec())
}

async fn read(response: reqwest::Response, what: &str) -> Result<Value> {
    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .with_context(|| format!("{what} failed"))?;
    tracing::debug!(what, status, bytes = text.len(), "response");
    if !(200..300).contains(&status) {
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("errorMessage")
                    .or_else(|| v.get("error"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();
        if detail.is_empty() {
            bail!("{what} failed (HTTP {status})");
        }
        bail!("{what} failed (HTTP {status}): {detail}");
    }
    if text.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).with_context(|| format!("{what} answered non-JSON"))
}

fn profile_when_present(body: &Value) -> Option<Profile> {
    if body.get("skins").map(Value::is_array) == Some(true) {
        Some(parse_profile(body))
    } else {
        tracing::warn!("skin change response carried no profile; refetching");
        None
    }
}

fn parse_profile(body: &Value) -> Profile {
    let skins = array(body, "skins")
        .filter_map(|skin| {
            let url = skin.get("url").and_then(Value::as_str)?.to_string();
            let key = skin
                .get("textureKey")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| texture_key_from_url(&url))?;
            Some(ProfileSkin {
                key,
                variant: match skin.get("variant").and_then(Value::as_str) {
                    Some("SLIM") => SkinVariant::Slim,
                    _ => SkinVariant::Classic,
                },
                active: is_active(skin),
                url,
            })
        })
        .collect();
    let capes = array(body, "capes")
        .filter_map(|cape| {
            Some(ProfileCape {
                id: cape.get("id").and_then(Value::as_str)?.to_string(),
                name: cape
                    .get("alias")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                url: cape.get("url").and_then(Value::as_str)?.to_string(),
                active: is_active(cape),
            })
        })
        .collect();
    Profile { skins, capes }
}

fn array<'a>(body: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    body.get(key)
        .and_then(Value::as_array)
        .map(|v| v.as_slice())
        .unwrap_or_default()
        .iter()
}

fn is_active(item: &Value) -> bool {
    item.get("state").and_then(Value::as_str) == Some("ACTIVE")
}

fn texture_key_from_url(url: &str) -> Option<String> {
    let key = url.rsplit('/').next()?;
    (!key.is_empty()).then(|| key.to_string())
}

fn variant_name(variant: SkinVariant) -> &'static str {
    match variant {
        SkinVariant::Classic => "classic",
        SkinVariant::Slim => "slim",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_profile_with_active_skin_and_cape() {
        let body: Value = serde_json::from_str(
            r#"{
                "id": "abc", "name": "Player",
                "skins": [
                    {"id": "s1", "state": "ACTIVE",
                     "url": "http://textures.minecraft.net/texture/aa11",
                     "variant": "SLIM"},
                    {"id": "s2", "state": "INACTIVE",
                     "url": "http://textures.minecraft.net/texture/bb22",
                     "textureKey": "bb22", "variant": "CLASSIC"}
                ],
                "capes": [
                    {"id": "c1", "state": "ACTIVE", "alias": "Migrator",
                     "url": "http://textures.minecraft.net/texture/cc33"}
                ]
            }"#,
        )
        .unwrap();
        let profile = parse_profile(&body);
        let active = profile.active_skin().unwrap();
        assert_eq!(active.key, "aa11");
        assert_eq!(active.variant, SkinVariant::Slim);
        assert_eq!(profile.skins.len(), 2);
        let cape = profile.capes.iter().find(|c| c.active).unwrap();
        assert_eq!(cape.name, "Migrator");
        assert_eq!(cape.id, "c1");
    }
}
