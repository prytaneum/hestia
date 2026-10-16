# A pack updates from a file, and leaves an entry two ways

*Applies to: [Content & modpacks](../architecture/content.md#modpacks)*

A pack no source published had no way forward. The update check answered
nothing, `fetch_update` refused it, and the one workaround — installing the newer
file into the same entry — was refused as soon as the file pinned another game
version, because only the update path moves an entry's version and it never took
a file. Taking a pack back out had the opposite problem: the only removal deleted
everything the pack had installed.

**An update takes a file in place of a version.** `ModpackMove` names a
published `version` or a `path`, and a file goes through the update that already
exists — the game-version move behind the downgrade gate, the stale-item sweep,
the keep-if-edited rule. Any pack accepts one, so a published pack can take a
pre-release build; a file no source recognises then leaves it unpublished, with
no remote updates until a recognised file is supplied.

**"The same pack" is the project, or failing that the name.** A `.mrpack` carries
a name and a free-text version and no id. Where a source recognises both sides
the project decides; otherwise the names are compared, ignoring case. A different
pack is refused as `ModpackDifferentPack` unless `allow_other_pack` says it is
meant — authors rename packs, so a refusal with no way past would block real
updates, and no check at all would let a wrong file swap the pack out.

**Two versions cannot be ordered, so only the game version is gated.** The
version string is free text. What can be compared is the game version each pack
pins, which is the downgrade gate every version move already uses.

**A front-end asks before the job, not after it fails.**
`modpack.preview_update` reads the file and answers the two questions —
same pack, downgrade — so the desktop and the CLI confirm once and then start a
job that will not be refused. The engine checks again regardless: the preview is
a courtesy, the refusal is the rule.

**Removal keeps the content, or does not.** `keep_content` drops the pack alone:
`modpack.json` is cleared and its pool items lose the origin tag, so they are
removable and syncable ([0072](0072-a-pack-syncs-by-identity.md)) like anything
the user installed. The files it wrote into the game directory stay and stop
being recorded. The CLI asks which when neither `--keep` nor `--all` is given.

**What other launchers do**, checked against their sources and issue trackers:

- Prism Launcher is the only one that updates from a file: an "Update from file"
  action on the instance's pack page, and an import whose pack name matches an
  existing instance offers to update it. Its source carries a FIXME for the case
  of two instances running the same pack.
- The Modrinth app does not: requests for it (modrinth/code #3670, #4158) are
  closed as duplicates of the open "Delta-import" request, #362.
- GDLauncher does not (gorilla-devs/GDLauncher #1404, #677).

**Rejected:** matching a file to an existing entry at install time, as Prism
does. A name is all there is to match on, and it cannot tell two entries running
the same pack apart; the user naming the entry cannot be wrong. **Also
rejected:** typed errors alone, with the front-end retrying on a refusal — every
ordinary confirmation would then show up as a failed job first.
