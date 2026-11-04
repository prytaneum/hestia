//! Remote images, fetched on the webview's behalf.

use proto::error::{ErrorInfo, Field};
use proto::image::{ImageFetch, ImageFetchParams};

use crate::runtime::Channels;

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<ImageFetch, _, _>(|p: ImageFetchParams, ctx| async move {
        if p.url.is_empty() {
            return Err(ErrorInfo::FieldRequired { field: Field::Url });
        }
        ctx.runtime
            .engine()
            .fetch_image(&p.url)
            .await
            .map_err(crate::runtime::engine_error)
    });
}
