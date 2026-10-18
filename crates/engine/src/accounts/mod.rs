//! Minecraft accounts signed in through Microsoft, persisted with their tokens
//! in `<data_home>/accounts.json` (owner-only on POSIX). Both sign-in methods
//! converge on the shared signed tail; tokens never leave the daemon.

mod microsoft;
mod signing;

pub(crate) use microsoft::USER_AGENT;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use common::now_unix as now_seconds;

use anyhow::{anyhow, bail, Result};
use proto::accounts::{Account, LoginMethod};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::checksum::hex;
use microsoft::{
    launcher_login, minecraft_profile, poll_device_code, redeem_code, refresh_oauth,
    request_device_code, request_device_token, sisu_authenticate, sisu_authorize, xsts_authorize,
    OAuthRejected, OAuthTokens,
};
use signing::{base64url_nopad, random_bytes, ProofKey};

use crate::schema::{self, Document};

const REFRESH_MARGIN_SECONDS: i64 = 300;

/// The account's refresh token was rejected; recover only by re-login. Mapped to
/// the `unauthorized` code so front-ends prompt sign-in instead of failing opaquely.
#[derive(Debug, thiserror::Error)]
#[error("your Microsoft sign-in for '{reference}' has expired; run `hestia account login` to sign in again")]
pub struct ReauthRequired {
    pub reference: String,
}

impl ReauthRequired {
    fn new(reference: &str) -> Self {
        ReauthRequired {
            reference: reference.to_string(),
        }
    }
}

/// A token for a launch, and whether the launcher could confirm it is current.
/// A stale one still opens singleplayer; the game refuses multiplayer with it.
pub struct AccessToken {
    pub token: String,
    pub stale: bool,
}

/// What the user must act on to finish a sign-in.
pub struct LoginChallenge {
    pub id: String,
    pub method: LoginMethod,
    pub url: String,
    pub user_code: String,
    pub verification_uri: String,
}

struct LoginSession {
    method: LoginMethod,
    key: Option<ProofKey>,
    device_token: String,
    verifier: String,
    session_id: String,
    device_code: String,
    interval_seconds: i64,
    expires_at: i64,
    clock_offset: i64,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct StoredAccount {
    uuid: String,
    name: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    expires_at: i64,
    /// Set when the refresh token is rejected; cleared by a fresh sign-in.
    #[serde(default)]
    needs_reauth: bool,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AccountsFile {
    #[serde(default)]
    accounts: Vec<StoredAccount>,
    /// The account launches use when none is named; empty falls back to the
    /// first signed-in one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    default_uuid: String,
}

impl Document for AccountsFile {
    const NAME: &'static str = "accounts.json";
}

pub struct Accounts {
    path: PathBuf,
    pending: Mutex<HashMap<String, LoginSession>>,
}

impl Accounts {
    pub fn new(path: PathBuf) -> Self {
        Accounts {
            path,
            pending: Mutex::new(HashMap::new()),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    pub fn list(&self) -> Vec<Account> {
        load(self.path())
            .accounts
            .into_iter()
            .map(|a| Account {
                uuid: a.uuid,
                name: a.name,
                needs_reauth: a.needs_reauth,
            })
            .collect()
    }

    pub fn has_account(&self) -> bool {
        !load(self.path()).accounts.is_empty()
    }

    /// Whether the resolved account (or the default, for an empty reference)
    /// needs a fresh sign-in. A missing account returns false — callers handle
    /// "no account" separately.
    pub fn needs_reauth(&self, reference: &str) -> bool {
        let file = load(self.path());
        let account = if reference.is_empty() {
            file.accounts
                .iter()
                .find(|a| a.uuid == file.default_uuid)
                .or_else(|| file.accounts.first())
        } else {
            file.accounts
                .iter()
                .find(|a| a.uuid == reference || a.name == reference)
        };
        account.is_some_and(|a| a.needs_reauth || a.refresh_token.is_empty())
    }

    /// The account launches use when none is named: the switched-to one, else
    /// the first signed-in one.
    pub fn default_account(&self) -> Option<Account> {
        let file = load(self.path());
        let stored = file
            .accounts
            .iter()
            .find(|a| a.uuid == file.default_uuid)
            .or_else(|| file.accounts.first())?;
        Some(Account {
            uuid: stored.uuid.clone(),
            name: stored.name.clone(),
            needs_reauth: stored.needs_reauth,
        })
    }

    /// Make `reference` (name or uuid) the default account. Returns `None` when
    /// no account matches.
    pub fn switch(&self, reference: &str) -> Result<Option<Account>> {
        let path = self.path();
        let mut file = load(path);
        let Some(chosen) = file
            .accounts
            .iter()
            .find(|a| a.uuid == reference || a.name == reference)
            .map(|a| Account {
                uuid: a.uuid.clone(),
                name: a.name.clone(),
                needs_reauth: a.needs_reauth,
            })
        else {
            return Ok(None);
        };
        file.default_uuid = chosen.uuid.clone();
        save(path, &file)?;
        tracing::info!(name = %chosen.name, "switched default account");
        Ok(Some(chosen))
    }

    pub async fn begin_login(&self, method: LoginMethod) -> Result<LoginChallenge> {
        let id = uuid::Uuid::new_v4().to_string();
        tracing::info!(?method, "starting sign-in");

        if method == LoginMethod::DeviceCode {
            let device = request_device_code().await?;
            let session = LoginSession {
                method,
                key: None,
                device_token: String::new(),
                verifier: String::new(),
                session_id: String::new(),
                device_code: device.device_code,
                interval_seconds: device.interval_seconds,
                expires_at: now_seconds() + device.expires_in_seconds,
                clock_offset: 0,
            };
            self.pending.lock().unwrap().insert(id.clone(), session);
            return Ok(LoginChallenge {
                id,
                method,
                url: String::new(),
                user_code: device.user_code,
                verification_uri: device.verification_uri,
            });
        }

        let key = ProofKey::generate();
        let device = request_device_token(&key).await?;
        let verifier = hex(&random_bytes(64));
        let challenge = base64url_nopad(&sha256_bytes(&verifier));
        let state = hex(&random_bytes(16));
        let auth =
            sisu_authenticate(&device.token, &challenge, &state, &key, device.clock_offset).await?;

        let session = LoginSession {
            method,
            key: Some(key),
            device_token: device.token,
            verifier,
            session_id: auth.session_id,
            device_code: String::new(),
            interval_seconds: 5,
            expires_at: 0,
            clock_offset: device.clock_offset,
        };
        self.pending.lock().unwrap().insert(id.clone(), session);
        Ok(LoginChallenge {
            id,
            method,
            url: auth.url,
            user_code: String::new(),
            verification_uri: String::new(),
        })
    }

    pub async fn complete_login(&self, id: &str, code: &str) -> Result<Account> {
        let session = {
            self.pending
                .lock()
                .unwrap()
                .remove(id)
                .ok_or_else(|| anyhow!("no sign-in is in progress for this request"))?
        };

        let (oauth, xsts) = if session.method == LoginMethod::DeviceCode {
            let oauth = await_device_tokens(&session).await?;
            let key = ProofKey::generate();
            let device = request_device_token(&key).await?;
            let authorization = sisu_authorize(
                "",
                &oauth.access_token,
                &device.token,
                &key,
                device.clock_offset,
            )
            .await?;
            let xsts =
                xsts_authorize(&authorization, &device.token, &key, device.clock_offset).await?;
            (oauth, xsts)
        } else {
            let oauth = redeem_code(code, &session.verifier).await?;
            let key = session
                .key
                .as_ref()
                .ok_or_else(|| anyhow!("sisu session lost its proof key"))?;
            let authorization = sisu_authorize(
                &session.session_id,
                &oauth.access_token,
                &session.device_token,
                key,
                session.clock_offset,
            )
            .await?;
            let xsts = xsts_authorize(
                &authorization,
                &session.device_token,
                key,
                session.clock_offset,
            )
            .await?;
            (oauth, xsts)
        };

        let minecraft = launcher_login(&xsts).await?;
        let profile = minecraft_profile(&minecraft.access_token).await?;

        let record = StoredAccount {
            uuid: profile.uuid.clone(),
            name: profile.name.clone(),
            refresh_token: oauth.refresh_token,
            access_token: minecraft.access_token,
            expires_at: now_seconds() + minecraft.expires_in,
            needs_reauth: false,
        };

        {
            let path = self.path();
            let mut file = load(path);
            file.accounts.retain(|a| a.uuid != record.uuid);
            file.accounts.push(record);
            save(path, &file)?;
        }
        tracing::info!(name = %profile.name, uuid = %profile.uuid, "signed in");
        Ok(Account {
            uuid: profile.uuid,
            name: profile.name,
            needs_reauth: false,
        })
    }

    /// A Minecraft access token for `reference` (uuid or name), rotating the
    /// stored tokens through Microsoft when they are at or near expiry.
    ///
    /// A rotation that cannot reach Microsoft answers with the stored token
    /// marked stale rather than failing: everything else about the launch is
    /// already on disk, so a dropped connection should not be what stops
    /// someone playing singleplayer.
    pub async fn access_token(&self, reference: &str) -> Result<AccessToken> {
        let path = self.path();
        let mut account = {
            let file = load(path);
            file.accounts
                .into_iter()
                .find(|a| a.uuid == reference || a.name == reference)
                .ok_or_else(|| {
                    anyhow::Error::from(proto::error::ErrorInfo::AccountNotFound {
                        reference: reference.to_string(),
                    })
                })?
        };

        if account.needs_reauth || account.refresh_token.is_empty() {
            return Err(ReauthRequired::new(reference).into());
        }
        if account.expires_at - now_seconds() > REFRESH_MARGIN_SECONDS {
            return Ok(AccessToken {
                token: account.access_token,
                stale: false,
            });
        }

        if let Err(e) = rotate_tokens(&mut account).await {
            // A rejected refresh token is fatal; a transient failure is not.
            if e.downcast_ref::<OAuthRejected>().is_some() {
                self.flag_reauth(&account.uuid)?;
                return Err(ReauthRequired::new(reference).into());
            }
            if crate::net::is_offline(&e) && !account.access_token.is_empty() {
                tracing::warn!(
                    uuid = %account.uuid,
                    "cannot reach microsoft to refresh; using the stored token"
                );
                return Ok(AccessToken {
                    token: account.access_token,
                    stale: true,
                });
            }
            return Err(e);
        }
        let mut file = load(path);
        if let Some(existing) = file.accounts.iter_mut().find(|a| a.uuid == account.uuid) {
            *existing = account.clone();
        } else {
            file.accounts.push(account.clone());
        }
        save(path, &file)?;
        Ok(AccessToken {
            token: account.access_token,
            stale: false,
        })
    }

    fn flag_reauth(&self, uuid: &str) -> Result<()> {
        let path = self.path();
        let mut file = load(path);
        if let Some(account) = file.accounts.iter_mut().find(|a| a.uuid == uuid) {
            account.needs_reauth = true;
            save(path, &file)?;
            tracing::warn!(uuid, "account refresh rejected; sign-in required");
        }
        Ok(())
    }

    pub fn remove(&self, reference: &str) -> Result<bool> {
        let path = self.path();
        let mut file = load(path);
        let before = file.accounts.len();
        file.accounts
            .retain(|a| a.uuid != reference && a.name != reference);
        if file.accounts.len() == before {
            return Ok(false);
        }
        if !file.accounts.iter().any(|a| a.uuid == file.default_uuid) {
            file.default_uuid = String::new();
        }
        save(path, &file)?;
        tracing::info!(reference, "signed out account");
        Ok(true)
    }
}

async fn await_device_tokens(session: &LoginSession) -> Result<OAuthTokens> {
    let interval = Duration::from_secs(session.interval_seconds.max(1) as u64);
    while now_seconds() < session.expires_at {
        if let Some(tokens) = poll_device_code(&session.device_code).await? {
            return Ok(tokens);
        }
        tokio::time::sleep(interval).await;
    }
    bail!(proto::error::ErrorInfo::LoginTimedOut)
}

async fn rotate_tokens(account: &mut StoredAccount) -> Result<()> {
    if account.refresh_token.is_empty() {
        bail!(proto::error::ErrorInfo::SessionExpired {
            reference: account.name.clone(),
        });
    }
    tracing::debug!(uuid = %account.uuid, "refreshing minecraft token");
    let oauth = refresh_oauth(&account.refresh_token).await?;
    let key = ProofKey::generate();
    let device = request_device_token(&key).await?;
    let authorization = sisu_authorize(
        "",
        &oauth.access_token,
        &device.token,
        &key,
        device.clock_offset,
    )
    .await?;
    let xsts = xsts_authorize(&authorization, &device.token, &key, device.clock_offset).await?;

    let minecraft = launcher_login(&xsts).await?;
    account.access_token = minecraft.access_token;
    if !oauth.refresh_token.is_empty() {
        account.refresh_token = oauth.refresh_token;
    }
    account.expires_at = now_seconds() + minecraft.expires_in;
    account.needs_reauth = false;
    Ok(())
}

fn sha256_bytes(text: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher.finalize().to_vec()
}

fn load(path: &Path) -> AccountsFile {
    schema::load(path).unwrap_or_default()
}

// The file holds tokens, so it is written owner-only — the mode lands on the
// temp file before the rename, so it is never briefly readable by anyone else.
fn save(path: &Path, file: &AccountsFile) -> Result<()> {
    schema::save_private(path, file)
}
