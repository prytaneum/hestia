//! A client that can only reach the public internet, for fetches whose URL
//! comes from someone else: a content description, a plugin.
//!
//! The check is made on the addresses the connection will actually use. A
//! resolver that vetted a name and let the connector resolve it again would be
//! open to a rebinding answer; here the vetted addresses are the ones dialled.
//! An IP-literal host never reaches a resolver, so callers refuse those up front
//! (see [`is_ip_literal`]).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::OnceLock;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(15);

/// A name resolved to an address inside this machine or its networks.
#[derive(Debug, thiserror::Error)]
#[error("{host} resolves to a non-public address")]
pub struct Refused {
    pub host: String,
}

/// Public means reachable from anywhere: not loopback, private, link-local,
/// carrier-grade NAT, multicast, reserved or unspecified. IPv6 forms that wrap
/// an IPv4 address are judged as that address.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (18..20).contains(&b))
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    let segments = ip.segments();
    // 64:ff9b::/96 translates to the IPv4 address in its low 32 bits.
    if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        let [high, low] = [segments[6], segments[7]];
        return is_public_v4(Ipv4Addr::new(
            (high >> 8) as u8,
            high as u8,
            (low >> 8) as u8,
            low as u8,
        ));
    }
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80)
}

pub fn is_ip_literal(host: &str) -> bool {
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .is_ok()
}

/// Every address must be public: one private answer among public ones is what a
/// rebinding attacker serves, and a connector would happily try it.
pub fn vet(host: &str, addrs: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, Refused> {
    if addrs.is_empty() || addrs.iter().any(|addr| !is_public(addr.ip())) {
        return Err(Refused {
            host: host.to_string(),
        });
    }
    Ok(addrs)
}

pub struct GuardedResolver;

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let found: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let vetted = vet(&host, found)?;
            Ok(Box::new(vetted.into_iter()) as Addrs)
        })
    }
}

/// Whether a failed request failed because [`GuardedResolver`] refused, which is
/// the answer, not a sign the network is down.
pub fn was_refused(error: &reqwest::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if current.is::<Refused>() {
            return true;
        }
        source = current.source();
    }
    false
}

/// The shared client for untrusted URLs. Redirects are not followed — the caller
/// walks them, so each hop is checked — and no proxy is used, since a proxy would
/// do the resolving and dialling this client exists to control. Cookies are off
/// because the `cookies` feature is not built.
pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(common::app::user_agent())
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .dns_resolver(GuardedResolver)
            .build()
            .expect("guarded reqwest client builds")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(ip: &str) -> SocketAddr {
        SocketAddr::new(ip.parse().expect("an ip"), 0)
    }

    #[test]
    fn only_publicly_routable_addresses_pass() {
        let refused = [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "64:ff9b::7f00:1",
        ];
        for ip in refused {
            assert!(
                !is_public(ip.parse().expect("an ip")),
                "{ip} must be refused"
            );
        }
        for ip in ["93.184.216.34", "2606:2800:220:1:248:1893:25c8:1946"] {
            assert!(is_public(ip.parse().expect("an ip")), "{ip} must pass");
        }
    }

    #[test]
    fn a_name_with_any_private_answer_is_refused() {
        assert!(vet("a.test", vec![addr("93.184.216.34")]).is_ok());
        assert!(vet("a.test", vec![addr("127.0.0.1")]).is_err());
        assert!(vet("a.test", vec![addr("93.184.216.34"), addr("10.0.0.1")]).is_err());
        assert!(vet("a.test", vec![]).is_err());
    }

    #[tokio::test]
    async fn the_client_cannot_be_pointed_at_this_machine_by_name() {
        let error = client()
            .get("https://localhost/")
            .send()
            .await
            .expect_err("localhost resolves to loopback");
        assert!(was_refused(&error), "{error:?}");
    }
}
