# The merge writes the game's own file, not its own rendering of it

*Applies to: [Instances](../architecture/entries.md#sync--shared-settings-across-instances)*

The `options.txt` merge parsed the file into a map and rendered the map back.
Everything the map could not hold was lost on the first pass: comments, blank
lines, any line a mod wrote that is not `key:value`, the original key order, and
the file's line endings. Because the rendering differed from what the game
wrote, every pass rewrote the file and restamped it, whether or not a value had
moved — and that stamp is what the tiebreak on the other side reads. A file the
launcher could not decode as UTF-8 read as an *empty* map and was then
overwritten from the shared copy, which is data loss rather than degradation.

The merge now works on a document that keeps the lines it read and rewrites only
the values it settles, and an unreadable file is an error that skips the unit
rather than an empty one that replaces it. `servers.dat` is merged as the rows
the file holds, so a tag this build does not model survives the round trip, and
each side keeps its own order with new rows appended.

**A row's identity comes from the agreement, not its name.** Keying on the
player's name for a row made a rename read as a delete plus an add: the old row
survived in every other instance and the renamed one arrived beside it. The
agreement already records what was last written for that instance, so a row is
paired against it by identical content, then by address, then by name, nearest
position breaking a tie — a rename keeps its identity through the address, an
address edit through the name.

Three smaller rules follow the same principle of writing what the game would.
The command history is capped at the 50 lines the game itself keeps, because an
uncapped union undid the game's own trim on every launch and grew without bound.
Keys that describe the file, the machine or a moment — `version`, `lastServer`,
the display and audio keys, the first-run prompts — never travel, since a copy
makes the receiving instance wrong rather than merely different; `version` in
particular tells a client its settings came from another game version and makes
it migrate them. And an instance's own copy is kept at
`<store>/.backups/<instance>/<file>` before sharing first lands on it, because
the first pass overwrites in place and nothing else held that file. It is one
copy per instance and unit, written through a temp and replaced on the next
enable; nothing reads it, so older generations would only accumulate.

Writing the game's own file is about the bytes around a value, not the value
itself: which settings exist and how each is spelled per version is the
catalogue's problem ([0071](0071-options-sync-through-a-typed-catalogue.md)).
