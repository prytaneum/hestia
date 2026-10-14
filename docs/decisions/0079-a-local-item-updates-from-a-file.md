# A local item updates from a file

*Applies to: [Content & modpacks](../architecture/content.md#beyond-add-and-remove)*

A local import had no way forward. A new build almost always carries a new
filename (`my-mod-1.0.jar` → `my-mod-1.1.jar`), and an add only replaces a row
with the same filename or the same project — a local item has no project — so
adding the build put a second copy beside the first, and both loaded. The way
through was remove-then-add, which also threw away how the entry held the item:
its enabled flag, its per-world disables, its world targeting.

**`update` takes a file in place of a source.** `<kind>.content.update` carries
a `path`; with one, the single named item moves onto that file instead of the
newest published version. It is the same verb a pack uses for the same need
([0078](0078-a-pack-updates-from-a-file.md)), and the verb `add --file` already
taught: the user is updating the item, wherever the bytes come from. The record
is moved with `record::resource`, so the holding is kept by construction
([0064](0064-a-record-is-mutated-not-rebuilt.md)).

**A file updates exactly one item.** A batch of names has nothing to pair the
file with; the daemon refuses it before the job starts (`FileUpdatesOneItem`).

**Only a local item takes one.** A tracked item updated from a file would lose
its source silently under a verb that promises to keep it current, so it is
refused (`FileUpdatesLocalOnly`) — moving a tracked item is `set_version`'s job.
A pack-owned item is refused too, as its removal is: the pack would put its own
file back at the next apply.

**The result stays local.** The user chose to keep the item untracked; the new
build is not looked up, and "Track on a source" adopts one afterwards if the user
wants ([0075](0075-a-local-file-is-identified-by-its-hash.md)).

**A rebuild under the same name is what the game loads next.** The mirror never
refreshes a path that already exists, so the old mirrors are dropped before the
new file is placed.

**Rejected:** a separate `replace` verb and contract. Its meaning was crisper,
but it was a second way to say "update", a new channel and facade method on each
side, for what one optional field on the existing channel already carries.
**Also rejected:** matching a dropped file to an installed one by the mod id in
its manifest. Only mods and plugins carry one, and a silent replacement chosen by
a heuristic is worse than asking the user to name the item.
