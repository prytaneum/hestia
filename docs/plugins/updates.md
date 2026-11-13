# Updates

Hestia checks for newer plugin builds in the background (a minute after the daemon starts, then every six hours) and
on demand. The result is the `plugin.updates` event and the `plugin.outdated` call; `hestia plugin outdated` prints it
and the desktop shows an Update button on each plugin and a count on the Installed tab.

## What is checked

| Installed from | Checked |
|---|---|
| A repository, or a link signed by a known key | Every configured repository pinning the key that signed the install and listing the plugin. One request per repository, shared by every plugin from it. |
| A link, unsigned | Only on request (`hestia plugin outdated --unsigned`): the file is downloaded again and its version compared. Never on the schedule. |
| A file, a folder or a dev folder | Never. |
| A plugin no configured repository lists any more (obsolete) | Never. |

The newest prebuilt build this Hestia can run, across the repositories that share the key, is the one offered. A newer
version that has no build yet is not reported: it appears once its author publishes one. Another key listing the same
plugin id is ignored.

## Plugin API compatibility

A build runs only when its `api` requirement is met by this Hestia's plugin API. When the latest version needs a newer
API, `requires` names that requirement (the index carries the API requirement, not a Hestia version) and `available` is
the newest build that does run, if it is newer than what is installed, with `apiCompatible: true`. When no newer build
runs, `available` is the latest version and `apiCompatible` is false.

## Permissions

`widened` lists the permissions the offered version asks for beyond what you approved, as its repository's index states
them. Applying the update shows the full comparison and asks for approval.

## Applying an update

`hestia plugin update <id>` (or `--all`), or the Update button, downloads the newest build and verifies it against the
key that signed the installed plugin; a build signed by any other key is refused, and so is one that is not newer.

If the new version asks for more than you approved, nothing changes until you approve exactly that grant: the CLI shows
it and asks (`-y` approves what is shown), the desktop shows it in the approval dialog. An update that asks for the
same or less applies without asking.

Your plugin's `data/` is kept. Hestia also keeps the one previous package. If the new version fails to initialise, it is
put back and the update reports `plugin_update_rolled_back`; if it traps or is disabled within its first minute it is rolled back too, and the daemon publishes
`plugin.rolled_back` (the id, the version taken out and the version restored), which the desktop shows as a toast.
Commands already running finish on the old code, and running `plugin.run` jobs are not interrupted.

## Offline

With the network down, or a repository unreachable, the answer is built from the last good copy of each index, and the
result says so (`offline`, plus a warning naming a repository that could not be refreshed).
