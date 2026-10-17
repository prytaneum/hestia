# The desktop bridge is one generic command, not a facade mirror

*Applies to: [Front-ends](../architecture/frontends.md)*

The intended recipe used to be one `#[tauri::command]` per feature calling a
client facade — a placeholder written before the shell was wired. Mirroring ~80
channels as Tauri commands would add a third naming seam (proto channel → Rust
command → TS wrapper) that can drift from both sides while adding no safety:
`invoke()` results are untyped JSON regardless, and the daemon already validates
every payload through the wire contract (`bad_request`, `unknown_channel`). So
the Rust shell is a thin pipe and the typed layer lives in TS, where the
frontend consumes it — adding a channel to the desktop is a TS one-liner, no
recompile. (That layer is checked against a channel table generated from the
contracts, so it restates nothing by hand:
[0084](0084-the-ts-channel-table-is-generated-from-the-contracts.md).) Forwarding *all* events over one unscoped listener likewise keeps the Rust
side generic: the desktop needs many concurrent subscribers (several jobs, live
logs, list invalidation), so multiplexing by topic and job id lives in the
frontend's event bus, where many subscribers are natural.
