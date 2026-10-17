use ipc::errors::IpcError;
use proto::skins::{
    Cape, CapeClear, CapeClearParams, CapeEquip, CapeEquipParams, Skin, SkinAdd, SkinAddParams,
    SkinList, SkinListParams, SkinRemove, SkinRemoveParams, SkinReset, SkinResetParams, SkinUpdate,
    SkinUpdateParams, SkinVariant,
};

use crate::session::Session;

pub struct Skins<'a> {
    pub(crate) session: &'a Session,
}

impl Skins<'_> {
    /// The account's skins (library + defaults + the equipped external one)
    /// and owned capes. `account` is a name or uuid; empty uses the default.
    pub async fn list(&self, account: &str) -> Result<(Vec<Skin>, Vec<Cape>), IpcError> {
        let params = SkinListParams {
            account: account.to_string(),
        };
        let result = self.session.call::<SkinList>(&params).await?;
        Ok((result.skins, result.capes))
    }

    /// Upload a skin PNG (base64), equip it, and save it to the library.
    pub async fn add(
        &self,
        account: &str,
        name: &str,
        variant: SkinVariant,
        data: &str,
    ) -> Result<Skin, IpcError> {
        let params = SkinAddParams {
            account: account.to_string(),
            name: name.to_string(),
            variant,
            data: data.to_string(),
        };
        Ok(self.session.call::<SkinAdd>(&params).await?.skin)
    }

    /// Reset the account to its uuid-derived default skin.
    pub async fn reset(&self, account: &str) -> Result<(), IpcError> {
        let params = SkinResetParams {
            account: account.to_string(),
        };
        self.session.call::<SkinReset>(&params).await?;
        Ok(())
    }

    /// Rewrite a library entry's label and variant. A variant change on the
    /// equipped skin is re-pushed to Mojang, hence the generous timeout.
    pub async fn update(
        &self,
        account: &str,
        key: &str,
        name: &str,
        variant: SkinVariant,
    ) -> Result<Skin, IpcError> {
        let params = SkinUpdateParams {
            account: account.to_string(),
            key: key.to_string(),
            name: name.to_string(),
            variant,
        };
        Ok(self.session.call::<SkinUpdate>(&params).await?.skin)
    }

    /// Remove a library entry; the equipped Mojang skin is untouched.
    pub async fn remove(&self, key: &str) -> Result<(), IpcError> {
        let params = SkinRemoveParams {
            key: key.to_string(),
        };
        self.session.call::<SkinRemove>(&params).await?;
        Ok(())
    }

    pub async fn equip_cape(&self, account: &str, cape: &str) -> Result<(), IpcError> {
        let params = CapeEquipParams {
            account: account.to_string(),
            cape: cape.to_string(),
        };
        self.session.call::<CapeEquip>(&params).await?;
        Ok(())
    }

    pub async fn clear_cape(&self, account: &str) -> Result<(), IpcError> {
        let params = CapeClearParams {
            account: account.to_string(),
        };
        self.session.call::<CapeClear>(&params).await?;
        Ok(())
    }
}
