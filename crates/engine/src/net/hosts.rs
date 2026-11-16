//! The hosts Hestia's own code reaches. Each core caller names its policy here,
//! so what the launcher may contact is one list, under the same vetting a
//! plugin's requests get.

use std::sync::{Arc, OnceLock};

use kernel::vetting::{Hosts, Policy};

fn granted(
    cell: &'static OnceLock<Arc<Policy>>,
    connect: &[&str],
    unhashed: &[&str],
) -> Arc<Policy> {
    cell.get_or_init(|| {
        let hosts = Hosts::new(connect, unhashed).expect("core hosts are valid");
        Arc::new(Policy::new(hosts))
    })
    .clone()
}

/// The vanilla game: Mojang's catalogue and the files it names, each carrying
/// the SHA-1 its manifest gives. The manifests themselves are fetched unhashed.
pub(crate) fn mojang() -> Arc<Policy> {
    static POLICY: OnceLock<Arc<Policy>> = OnceLock::new();
    granted(
        &POLICY,
        &[
            "piston-meta.mojang.com",
            "piston-data.mojang.com",
            "launchermeta.mojang.com",
            "launcher.mojang.com",
            "libraries.minecraft.net",
            "resources.download.minecraft.net",
        ],
        &["piston-meta.mojang.com", "launchermeta.mojang.com"],
    )
}

/// The Bedrock Dedicated Server catalogue, Mojang's service and its mirror.
pub(crate) fn bedrock_catalogue() -> Arc<Policy> {
    static POLICY: OnceLock<Arc<Policy>> = OnceLock::new();
    let hosts = [
        "net-secondary.web.minecraft-services.net",
        "raw.githubusercontent.com",
    ];
    granted(&POLICY, &hosts, &hosts)
}

/// The Bedrock Dedicated Server archives, which carry no checksum.
pub(crate) fn bedrock() -> Arc<Policy> {
    static POLICY: OnceLock<Arc<Policy>> = OnceLock::new();
    let hosts = ["www.minecraft.net"];
    granted(&POLICY, &hosts, &hosts)
}

/// Sign-in and the profile: Microsoft, Xbox and Minecraft services, all API calls.
pub(crate) fn accounts() -> Arc<Policy> {
    static POLICY: OnceLock<Arc<Policy>> = OnceLock::new();
    let hosts = [
        "login.live.com",
        "device.auth.xboxlive.com",
        "sisu.xboxlive.com",
        "xsts.auth.xboxlive.com",
        "api.minecraftservices.com",
        "textures.minecraft.net",
    ];
    granted(&POLICY, &hosts, &hosts)
}

/// Eclipse Temurin: the API answers unhashed, the archives it links carry a SHA-256.
pub(crate) fn adoptium() -> Arc<Policy> {
    static POLICY: OnceLock<Arc<Policy>> = OnceLock::new();
    granted(
        &POLICY,
        &[
            "api.adoptium.net",
            "github.com",
            "objects.githubusercontent.com",
            "release-assets.githubusercontent.com",
        ],
        &["api.adoptium.net"],
    )
}

/// Self-update: the release feed and the artifacts it links, whose minisign
/// signature is checked after the download.
pub(crate) fn release() -> Arc<Policy> {
    static POLICY: OnceLock<Arc<Policy>> = OnceLock::new();
    let hosts = [
        "api.hestia.prytaneum.dev",
        "github.com",
        "objects.githubusercontent.com",
        "release-assets.githubusercontent.com",
    ];
    granted(&POLICY, &hosts, &hosts)
}

/// A page or file whose URL the user typed: any public host, and nothing to verify it by.
pub(crate) fn typed() -> Arc<Policy> {
    static POLICY: OnceLock<Arc<Policy>> = OnceLock::new();
    POLICY
        .get_or_init(|| Arc::new(Policy::new(Hosts::open_unhashed())))
        .clone()
}

/// A file an archive named: any public host, and a checksum is required.
pub(crate) fn untrusted() -> Arc<Policy> {
    static POLICY: OnceLock<Arc<Policy>> = OnceLock::new();
    POLICY
        .get_or_init(|| Arc::new(Policy::new(Hosts::open())))
        .clone()
}
