# Everything serialized is camelCase, except the `config.*` key vocabulary and upstream DTOs

*Applies to: [The socket boundary](../architecture/wire.md)*

The wire is camelCase (`proto` structs carry `rename_all = "camelCase"`;
`tests/casing.rs` enforces it), and the persisted engine records follow suit so
on-disk JSON is uniformly camelCase (`ServerRecord`, `InstanceRecord`, accounts,
`content.json`, the settings — Rust field names stay `snake_case`, only the
serialized form is renamed). Two deliberate exceptions, neither machine-enforced
in `engine` because a blanket guard would fight them: (1) the **`config.*` keys
are a stable kebab-case CLI vocabulary** (`memory`, `jvm-args`,
`defaults.jvm-args`) — the per-entry
`JavaSettings` decouples them with explicit key constants, and
the global `Settings` navigation translates each dotted segment through
`naming::config_key_to_field` (and `settings_to_config_keys` for the `config
list` view), so a user keeps typing kebab while `config.json` stores `jvmArgs`;
(2) the **upstream DTOs** (Adoptium, Mojang, Fabric, Modrinth, Microsoft) keep
whatever casing the remote API uses, since they deserialize *its* JSON, not
ours. The proto guard covers the one contract that matters — the socket; engine
record casing is a convention, not a lint, precisely because the DTOs next to
those records legitimately aren't camelCase.
