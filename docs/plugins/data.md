# Data

A plugin's state lives in its own `data/` directory, mounted at `/data`: the only place state outlives a call, since
every call runs on a fresh instance. `hestia::data` keeps typed JSON files there, named by a key of 1 to 64 of
`A-Z a-z 0-9 - _` and stored as `<key>.json`. The sandbox sees nothing else.

```rust
let mut counter: Counter = hestia::data::load("counter")?.unwrap_or_default();
counter.runs += 1;
hestia::data::save("counter", &counter)?;
```

`save` writes the value aside and renames it into place, so a reader never sees half a file.

## Changing a value safely

Calls of one plugin can overlap: an event handler and a command run at once, each on its own instance. Two calls that
`load`, change and `save` the same key can lose an update, because the last writer wins. `data::update` does the
read-modify-write under a lock:

```rust
let runs = hestia::data::update("counter", |counter: &mut Counter| {
    counter.runs += 1;
    counter.runs
})?;
```

`update` loads the value (or its `Default`), lets the closure change it, saves it, and returns what the closure
returned. Two `update`s of the same key never interleave: the second waits for the first, so no update is lost.

- **The host holds the lock.** WASI preview 2 has no file locks, so `update` asks the host for a lock named like the key
  (`data.lock` in `crates/sdk/wit/data.wit`) and gives it back when it is done. A lock belongs to the plugin; nothing
  else can take it.
- **A crashed call cannot wedge the plugin.** The lock is also released when the call that took it ends, whether it
  returned, trapped, timed out or was cancelled.
- **A wait is bounded.** A call waits a few seconds for another to let go, and never past its own deadline, then fails
  with `Error::Busy`. Keep the closure quick, since it holds everyone else up.
- **Not re-entrant.** Calling `update` on the same key from inside its closure is `Error::Invalid`; different keys
  nest. A call holds at most 8 locks and a plugin uses at most 64 names.
- **Plain `save` is not locked.** It stays last-writer-wins, so a key that is updated concurrently should only be
  written through `update`.
