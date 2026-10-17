//! Microsoft sign-in and the stored Minecraft accounts.

use proto::accounts::{
    AccountList, AccountListResult, AccountLoginBegin, AccountLoginBeginResult,
    AccountLoginComplete, AccountRemove, AccountSwitch,
};
use proto::error::ErrorInfo;
use proto::Empty;

use crate::runtime::Channels;

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<AccountLoginBegin, _, _>(|p, ctx| async move {
        tracing::info!(method = ?p.method, "account login started");
        let challenge = ctx
            .runtime
            .engine()
            .accounts()
            .begin_login(p.method)
            .await
            .map_err(crate::runtime::engine_error)?;
        Ok(AccountLoginBeginResult {
            id: challenge.id,
            method: challenge.method,
            url: challenge.url,
            user_code: challenge.user_code,
            verification_uri: challenge.verification_uri,
        })
    });

    on.handle::<AccountLoginComplete, _, _>(|p, ctx| async move {
        let account = ctx
            .runtime
            .engine()
            .accounts()
            .complete_login(&p.id, &p.code)
            .await
            .map_err(crate::runtime::engine_error)?;
        tracing::info!(account = %account.name, "account signed in");
        Ok(account)
    });

    on.handle::<AccountList, _, _>(|_: Empty, ctx| async move {
        let accounts = ctx.runtime.engine().accounts();
        Ok(AccountListResult {
            accounts: accounts.list(),
            default_uuid: accounts
                .default_account()
                .map(|a| a.uuid)
                .unwrap_or_default(),
        })
    });

    on.handle::<AccountSwitch, _, _>(|p, ctx| async move {
        match ctx.runtime.engine().accounts().switch(&p.account) {
            Ok(Some(account)) => {
                tracing::info!(account = %account.name, "default account switched");
                Ok(account)
            }
            Ok(None) => Err(ErrorInfo::AccountNotFound {
                reference: p.account.clone(),
            }),
            Err(e) => Err(crate::runtime::engine_error(e)),
        }
    });

    on.handle::<AccountRemove, _, _>(|p, ctx| async move {
        match ctx.runtime.engine().accounts().remove(&p.account) {
            Ok(true) => {
                tracing::info!(account = %p.account, "account removed");
                Ok(Empty {})
            }
            Ok(false) => Err(ErrorInfo::AccountNotFound {
                reference: p.account.clone(),
            }),
            Err(e) => Err(crate::runtime::engine_error(e)),
        }
    });
}
