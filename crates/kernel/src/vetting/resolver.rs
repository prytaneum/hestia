use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// A name resolved to an address inside this machine or its networks.
#[derive(Debug, thiserror::Error)]
#[error("{host} resolves to a non-public address")]
pub struct NonPublic {
    pub host: String,
}

/// Public means reachable from anywhere: not loopback, private, link-local,
/// carrier-grade NAT, multicast, reserved or unspecified. IPv6 forms that wrap
/// an IPv4 address are judged as that address.
fn is_public(ip: IpAddr) -> bool {
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
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (18..20).contains(&b))
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    let segments = ip.segments();
    let low_v4 = || {
        let [high, low] = [segments[6], segments[7]];
        Ipv4Addr::new((high >> 8) as u8, high as u8, (low >> 8) as u8, low as u8)
    };
    // These carry an IPv4 address in their low 32 bits: IPv4-compatible
    // (::/96) and NAT64 (64:ff9b::/96).
    if segments[..6] == [0, 0, 0, 0, 0, 0] && !ip.is_unspecified() && !ip.is_loopback() {
        return is_public_v4(low_v4());
    }
    if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return is_public_v4(low_v4());
    }
    // 6to4 (2002::/16) carries one in its second and third segments.
    if segments[0] == 0x2002 {
        let [high, low] = [segments[1], segments[2]];
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
        || (segments[0] & 0xffc0) == 0xfe80
        || (segments[0] & 0xffc0) == 0xfec0
        || segments[..3] == [0x64, 0xff9b, 0x0001]
        || (segments[0] == 0x0100 && segments[1..4] == [0, 0, 0])
        || (segments[0] == 0x2001
            && (segments[1] == 0 || segments[1] == 0x0db8 || (segments[1] & 0xfff0) == 0x0010)))
}

/// Every address must be public: one private answer among public ones is what a
/// rebinding attacker serves, and a connector would happily try it.
fn vet(host: &str, addrs: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, NonPublic> {
    if addrs.is_empty() || addrs.iter().any(|addr| !is_public(addr.ip())) {
        return Err(NonPublic {
            host: host.to_string(),
        });
    }
    Ok(addrs)
}

/// The check is made on the addresses the connection will use: a resolver that
/// vetted a name and let the connector resolve it again would be open to a
/// rebinding answer.
#[derive(Default)]
pub(super) struct Guarded {
    trusted: Option<String>,
}

impl Guarded {
    /// Resolves `host`, the user's own proxy, without vetting its addresses.
    pub fn trusting(host: Option<String>) -> Self {
        Guarded {
            trusted: host.map(|host| host.to_ascii_lowercase()),
        }
    }
}

impl Resolve for Guarded {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let trusted = self.trusted.as_deref() == Some(host.to_ascii_lowercase().as_str());
        Box::pin(async move {
            let found: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let found = if trusted { found } else { vet(&host, found)? };
            Ok(Box::new(found.into_iter()) as Addrs)
        })
    }
}

/// Whether a failed request failed because the resolver refused, which is the
/// answer, not a sign the network is down.
pub fn was_refused(error: &reqwest::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if current.is::<NonPublic>() {
            return true;
        }
        source = current.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(ip: &str) -> SocketAddr {
        SocketAddr::new(ip.parse().expect("an ip"), 0)
    }

    #[test]
    fn only_publicly_routable_addresses_pass() {
        for ip in [
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
            "::7f00:1",
            "::a00:1",
            "2002:7f00:1::1",
            "2002:c0a8:101::1",
            "fec0::1",
            "64:ff9b:1::1",
            "192.88.99.1",
            "2001:db8::1",
            "2001:0:4136:e378:8000:63bf:3fff:fdd2",
            "100::1",
            "2001:10::1",
        ] {
            assert!(
                vet("a.test", vec![addr(ip)]).is_err(),
                "{ip} must be refused"
            );
        }
        for ip in [
            "93.184.216.34",
            "2606:2800:220:1:248:1893:25c8:1946",
            "2002:5db8:d822::1",
            "2001:4860:4860::8888",
        ] {
            assert!(vet("a.test", vec![addr(ip)]).is_ok(), "{ip} must pass");
        }
        assert!(vet("a.test", vec![addr("93.184.216.34"), addr("10.0.0.1")]).is_err());
        assert!(vet("a.test", vec![]).is_err());
    }
}
