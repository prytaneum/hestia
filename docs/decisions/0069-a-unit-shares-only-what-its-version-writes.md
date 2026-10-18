# A unit shares only what its game version writes

*Applies to: [Instances](../architecture/entries.md#sync--shared-settings-across-instances)*

Only `options.txt` was gated on a game version — a pre-1.13 instance kept its
own, because 1.13 respelled every keybind. The other three units were treated as
era-agnostic, which was never true of two of them. `command_history.txt` did not
exist before 1.20.2, so an older instance was handed a file it will never read
and never write back. `hotbar.nbt` arrived in 1.12, and 1.20.5 replaced an item's
tags with components — a hotbar saved either side of that break is not a file the
other client can read, so the whole-file copy that was running put a modern save
into an old instance and called it shared.

Each unit now declares the version that first writes its file in a spelling the
others can read: options at 1.13, hotbars at 1.12, the command history at
1.20.2, the multiplayer list at none. An instance below the floor is skipped with
a warning naming the version that would share it — the `EraBound` state and its
warning became `Unsupported`, carrying `requires`, because the rule is no longer
about one era boundary.

The component break is not a floor, because both sides are still worth sharing
among themselves. Hotbars are stored per **era** instead: `<store>/legacy/` and
`<store>/components/`, each with its own agreements, so 1.20.4 instances share
with each other and 1.21 instances with each other, and neither ever writes a
file the other cannot read. A unit with one form keeps its copy where it was, so
nothing moved for the other three.

**Rejected:** translating between eras, the way a launcher with a per-setting
catalogue can. Converting an item stack between tags and components is lossy in
one direction and guesswork in the other, and a hotbar is not worth a converter
that silently drops what it cannot map. **Also rejected:** gating hotbars at
1.20.5 and abandoning older instances, which would stop sharing for the era that
has the most instances behind it.

## Amended: options has no floor

`options.txt` is no longer gated at 1.13. The options catalogue translates the
keybind respelling in both directions, so options share across every version;
the floors that remain are hotbars at 1.12 and the command history at 1.20.2.
