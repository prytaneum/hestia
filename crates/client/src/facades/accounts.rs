use ipc::errors::IpcError;
use proto::accounts::{
    Account, AccountList, AccountListResult, AccountLoginBegin, AccountLoginBeginParams,
    AccountLoginBeginResult, AccountLoginComplete, AccountLoginCompleteParams, AccountRemove,
    AccountRemoveParams, AccountSwitch, AccountSwitchParams, LoginMethod,
};

use crate::session::Session;

pub struct Accounts<'a> {
    pub(crate) session: &'a Session,
}

impl Accounts<'_> {
    /// Begin a sign-in; returns what the user must act on (a device code or a
    /// browser URL). The daemon holds per-login state keyed by the returned id.
    pub async fn begin_login(
        &self,
        method: LoginMethod,
    ) -> Result<AccountLoginBeginResult, IpcError> {
        self.session
            .call::<AccountLoginBegin>(&AccountLoginBeginParams { method })
            .await
    }

    /// Drive a begun sign-in to a stored account. Long-running (the device-code
    /// flow polls until the user approves), so it carries a generous timeout.
    pub async fn complete_login(&self, id: &str, code: &str) -> Result<Account, IpcError> {
        let params = AccountLoginCompleteParams {
            id: id.to_string(),
            code: code.to_string(),
        };
        Ok(self
            .session
            .call::<AccountLoginComplete>(&params)
            .await?
            .account)
    }

    /// The signed-in accounts plus the uuid launches default to.
    pub async fn list(&self) -> Result<AccountListResult, IpcError> {
        self.session.call::<AccountList>(&proto::Empty {}).await
    }

    /// Make `reference` (name or uuid) the default account for launches.
    pub async fn switch(&self, reference: &str) -> Result<Account, IpcError> {
        let params = AccountSwitchParams {
            account: reference.to_string(),
        };
        Ok(self.session.call::<AccountSwitch>(&params).await?.account)
    }

    pub async fn remove(&self, reference: &str) -> Result<(), IpcError> {
        let params = AccountRemoveParams {
            account: reference.to_string(),
        };
        self.session.call::<AccountRemove>(&params).await?;
        Ok(())
    }
}
