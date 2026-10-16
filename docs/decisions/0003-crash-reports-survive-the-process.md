# A crash must survive the process that had no console

*Applies to: [Cross-cutting foundations](../architecture/common.md)*

The daemon's stderr is detached, a release desktop build has `windows_subsystem
= "windows"`, and a panic inside a spawned task printed where nobody looks — so
a crash left nothing but a missing process. `init_logging` now installs the hook
for every binary (rather than each `main` remembering to), and reports from all
four share one directory. The webview reaches the same reports through `crash_report`, since a React
render error or an unhandled rejection kills the UI without touching the Rust
stack. Note that `panic = "abort"` with `strip = true` in the release profile
leaves release backtraces as bare addresses.
