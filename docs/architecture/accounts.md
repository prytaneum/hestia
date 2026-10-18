# Accounts & skins

*[← Architecture](../architecture.md)*

Playing Minecraft requires a Microsoft account that owns it. Hestia signs in
through Microsoft's own flows, keeps the tokens in the daemon, and never lets
them cross the socket.

A stored account is also a proof of ownership, which is why the whole
`instance.*` surface is refused until one exists — enforced once, at the router
([0030](../decisions/0030-instance-surface-gated-on-an-account.md)).

## Sign-in

Accounts live in `<data_home>/accounts.json`, owner-only on POSIX. Both methods
use the well-known Minecraft client id, so no per-distribution Azure app is
needed.

Sign-in is two steps, so a front-end can render whatever the user must act on:

```mermaid
sequenceDiagram
    autonumber
    participant F as front-end
    participant D as daemon
    participant MS as Microsoft
    participant XB as Xbox Live
    participant MC as Minecraft services

    F->>D: account.login.begin { method }
    alt device_code (CLI default)
        D->>MS: device-code grant
        MS-->>D: user_code + verification_uri
        D-->>F: show this code at this URL
        F->>D: account.login.complete
        D->>MS: poll until approved
    else sisu (desktop)
        D->>D: mint an ECDSA P-256 proof key
        D->>XB: PKCE sisu /authenticate
        XB-->>D: Microsoft sign-in URL
        D-->>F: open this URL
        F->>D: account.login.complete { code from the redirect }
        D->>MS: redeem the OAuth code
    end

    Note over D,MC: both converge on the same signed tail
    D->>XB: Xbox device token → sisu /authorize → XSTS
    D->>MC: launcher/login → profile
    D-->>F: stored account { uuid, name }
```

`access_token()` runs the same signed tail to rotate a token, but only when it is
within 5 minutes of expiry; when Microsoft is unreachable it yields the stored
token flagged stale. The front-end never sees a token.

- `accounts/microsoft.rs` — the HTTP steps (private to the module).
- `accounts/signing.rs` — Xbox request signing: the proof key and the
  FILETIME-stamped `Signature` header, in one cross-platform `p256`
  implementation rather than an OpenSSL/CNG split.

`account.switch` picks the default account launches run as; `account.list`
reports it. The desktop renders each account's player head from that account's
equipped skin texture — blitted locally, so an equip shows at once — and falls
back to the public `api.mineatar.io/face/<uuid>` service, derived from the uuid
rather than round-tripped over the wire, when no texture is loaded yet.

## The skin library

`<data_home>/skins/` holds PNG textures as `<key>.png` blobs beside a
`library.json` index — the disk is the registry, as everywhere else. A row is
keyed by **Mojang's texture hash**, so matching the account's currently equipped
skin at list time is a key comparison rather than an image diff.

```mermaid
flowchart LR
    LIB["library.json<br/><i>textures you saved</i>"] --> MERGED["one skin.list answer"]
    DEF["defaults.rs<br/><i>18 vanilla skins,<br/>by public texture URL</i>"] --> MERGED
    EXT["whatever the account<br/>currently wears"] --> MERGED
    MERGED --> UI["the desktop picker"]
    UI -->|equip / reset| MOJ["api.minecraftservices.com"]
    MOJ -->|"minted key"| LIB
```

**Before any change, the currently equipped skin is preserved** into the library
if neither it nor a default already records it — switching away from an
externally-set skin must never lose it.

The eighteen vanilla defaults (nine characters × two model variants) are listed
by their public texture URLs rather than bundled as PNGs, since Mojang serves
every texture publicly by hash. Equipping one is a by-URL skin change.

`skins/mojang.rs` holds the profile-customization calls — profile fetch,
multipart skin upload, by-URL change, reset, cape set and clear — bearer-authed
with the accounts subsystem's rotated token. A 30-second per-account profile
cache absorbs bursts of `skin.list` reads, because Mojang rate-limits hard; a
change stores the profile its response carries, or drops the entry so the next
read refetches.

Two deliberate departures from the launcher this design otherwise follows:

- **The library is global, not per-account.** A texture is not an entitlement,
  and the equipped state is per-account already.
- **A cape is not bound to a skin.** Mojang's own API models them independently
  (`skins/active` vs `capes/active`), and binding them is what forces a
  reconciliation dance nobody needs.

Changes apply immediately — the daemon is resident, so there is no app-close edge
to flush ([0017](../decisions/0017-skins-follow-modrinth-minus-couplings.md)).

**Skins are a desktop surface only.** Picking a skin is visual, so the CLI
deliberately grows no command for it — one of the documented exceptions to
[every capability getting a verb](../decisions/0041-every-capability-gets-a-verb.md).

## Decisions

- [0017 — Skins follow Modrinth's shape, minus its couplings](../decisions/0017-skins-follow-modrinth-minus-couplings.md)
