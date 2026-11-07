# A dev build resolves features once, and keeps debug info out of the binaries

*Applies to: [Cross-cutting foundations](../architecture/common.md), [Build & run](../contributing.md#build--run)*

The plugin runtime ([0091](0091-plugins-are-wasm-components.md)) brought wasmtime and Cranelift into the engine, so
`hestiad` and every test binary linking the engine grew from about 250 MB to 500–650 MB, almost all of it DWARF. Two
things multiplied that cost. Cargo builds a dependency once per distinct feature set, and every package selection
(`-p cli`, `-p daemon -p tray`, `--workspace`, `cargo test`) resolved a different one for crates beneath wasmtime
(`rustix`, `hashbrown`, `mio`, `anyhow`), so each compiled its own wasmtime, Cranelift and engine. Each `tests/*.rs`
was also its own binary, each relinking all of it.

**One package set is the default.** `default-members` is every crate but `desktop`, which `cargo tauri` builds on its
own and which never links the engine. A bare `cargo build`, `cargo test` or `cargo clippy` at the root, the scripts,
the pre-push hook and CI all resolve that set, so wasmtime compiles once per profile. The engine's tests only need WAT
text, so they parse it with the `wat` crate rather than enabling wasmtime's `wat` feature, which would otherwise split
the test build from the normal one. A `-p` selection still resolves its own features; narrow a test run by name
instead (`cargo test plugins::`).

**Debug info is split and trimmed.** `split-debuginfo = "unpacked"` leaves DWARF in per-object `.dwo` files instead of
copying it into each binary; Cargo drops the setting where rustc does not support it (Windows MSVC already writes a
`.pdb`). Dependencies keep `line-tables-only`, so a backtrace still resolves file and line inside them; the
workspace's own crates keep full debug info. The engine's integration tests are one `integration` binary, and the
desktop crate builds only an `rlib`, not the iOS `staticlib` and Android `cdylib` of the Tauri template.

Rejected: a `cargo-hakari` workspace-hack, which unifies features across every selection but has to be regenerated on
each dependency bump and so fights Dependabot (Zed removed theirs for the same reason); and Cargo's own
`resolver.feature-unification = "workspace"`, which does the same natively but is still nightly-only
([rust-lang/cargo#14774](https://github.com/rust-lang/cargo/issues/14774)). Revisit the latter once it is stable.
