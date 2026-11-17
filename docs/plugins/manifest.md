# The manifest

*[← Writing a plugin](README.md)*

`plugin.toml` holds what the component cannot say about itself. Permissions and exports are never declared here: Hestia
reads them from the component's imports and exports, so the manifest cannot ask for less or more than the code uses.
Keys it does not know are an error.

```toml
id = "tidy"
name = "Tidy"
version = "0.1.0"
api = "0.1.0"

[permissions."network:connect"]
hosts = ["api.example.com", "*.cdn.example.com"]
unhashed = ["api.example.com"]

[permissions."process:spawn"]
programs = [
    { path = "/usr/bin/rsync", args = ["-a", "--delete"], os = "linux" },
    { path = 'C:\Tools\robocopy.exe', args = ["/MIR"], os = "windows" },
]

[permissions."ipc:connect"]
endpoints = ["/run/tidy.sock", 'tidy-pipe']

[permissions.secrets]
optional = true

[[commands]]
name = "clean"
summary = "Delete old files"
args = [{ name = "keep", kind = "integer", required = true }, { name = "dry-run", kind = "boolean" }]

[[schedules]]
id = "nightly"
every = "1d"

[dependencies]
other-plugin = "^1.2"
```

## Identity

| Key       | Rule                                                                                            |
|-----------|-------------------------------------------------------------------------------------------------|
| `id`      | 1-64 lowercase letters, digits or `-`, not starting with `-`, not a Windows device name (`con`) |
| `name`    | 1-64 characters, no control or direction characters                                             |
| `version` | 1-32 letters, digits or `.+-`; the update check compares it                                     |
| `api`     | optional: the `hestia:kernel` version it was built against; `pack` writes it                    |

An `id` is one namespace with the built-in plugins, and it may not equal a `hestia` command (`server`, `plugin`, ...),
so `hestia <id> <command>` is never ambiguous. If `api` is set it must be the version the component imports; without it
only the imports are checked.

## Permission scopes

A `[permissions."<name>"]` table gives the scope of a permission that takes one. A scope for a permission the component
does not use, or a missing scope for one it does, is an error, and so is `optional` on a permission that cannot be
declined (`ipc:connect`, and `network:connect` for a plugin that imports `wasi:http`). See [Permissions](permissions.md)
for what each means.

| Key         | For                                                                                                |
|-------------|----------------------------------------------------------------------------------------------------|
| `optional`  | a permission the component needs and whose interfaces can answer `denied`; the user may decline it |
| `hosts`     | `network:connect`: exact names or `*.domain` (one label)                                           |
| `unhashed`  | `network:connect`: the `hosts` allowed to serve a download with no checksum                        |
| `endpoints` | `ipc:connect`: exact unix socket paths or Windows pipe names                                       |
| `programs`  | `process:spawn`: `{ path, args, os }` invocations                                                  |

Each list holds at most 64 values of at most 255 characters, a program at most 32 arguments. Everything shown in the
prompt must be plain text: control, zero-width and direction characters are refused.

## Commands and schedules

`[[commands]]` and `[[schedules]]` are covered in [Running work](commands.md). At most 32 commands (each with at most 16
arguments) and 8 schedules. Names are 1-32 lowercase letters, digits or `-`. No command is named `help` and no argument
`help` or `version`, which the CLI's parser keeps.

## Dependencies

`[dependencies]` names other plugins, built-ins included, as `id = "^1.2"` (caret ranges only, at most 16, never the
plugin itself). They are for a plugin whose contribution is needed but whose API it does not call. A package a component
imports is a requirement on its provider without any declaration. At install, Hestia resolves both before running any
code: each dependency must be installed, enabled and at an accepted version, or be listed by an added repository, which
then joins the plan. A cycle, a chain over 8, or a plan of more than 32 plugins is refused. One prompt shows every
plugin the install would bring with its signer and grant.

## Limits

The manifest is read as a regular file of at most 64 KiB and `plugin.wasm` of at most 64 MiB.
