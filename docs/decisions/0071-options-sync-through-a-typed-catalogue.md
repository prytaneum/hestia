# Options sync through a typed catalogue, not raw key-value pairs

*Applies to: [Instances](../architecture/entries.md#sync--shared-settings-across-instances)*

The options merge settled raw `key:value` pairs. Two instances on different game
versions therefore stopped sharing a setting the moment Mojang renamed its key,
and neither side could tell: `fancyGraphics` (≤1.15.2) became `graphicsMode`
(1.16–1.21.10) became `graphicsPreset` (1.21.11+), so each instance kept writing
a key the other ignored, and both files accumulated the spellings of every era
they had passed through. The 1.13 keybind respelling was the same problem at a
scale that could not be ignored, and it was met by refusing to share options with
a pre-1.13 instance at all ([0069](0069-a-unit-shares-only-what-its-version-writes.md)).

A shared setting is now a **canonical value under a stable id**, not a line of a
file. A catalogue entry names the physical keys the setting has had, the version
range each key is valid in, and how its value is encoded in that range. Reading
an instance decodes its file into canonical values; writing re-encodes each one
for that instance's own version. The shared copy is `<shared>/options.txt` — the
canonical values, spelled for the newest version — and every instance keeps its
own `options.txt`, still written as the game wrote it
([0070](0070-the-merge-writes-the-games-own-file.md)).

Two consequences follow. **The options floor goes away**: a legacy keybind is a
number that maps to a `key.keyboard.*` name in both directions, so a 1.12
instance shares with a 1.21 one. And a value the target version cannot express is
simply not written there, rather than written wrongly.

**A key the catalogue does not know is still shared.** Mods write their own
settings, and refusing to carry them would make the catalogue a whitelist of
vanilla. An unknown key travels as its raw string under its own id, and reaches
only instances that already have that key — an absent one means the mod is not
installed there, and inventing the line would be writing another mod's
configuration.

**A catalogue entry carries whether it shares by default.** Most settings are a
preference the player holds once — brightness, chat, sound, keybinds — and a few
describe the machine they are sitting at, where sharing is defensible but wrong
as a default: render distance, and anything else a weaker GPU has to be allowed
to disagree about. An unknown key defaults to shared, because a mod's setting is
a preference until proven otherwise, and the user turns any of it off per key or
per instance.

The keycode table and the list of keys the game itself writes are ported from
Modrinth's launcher (GPL-3.0-only), as the skin preview already is, and so is
the set of settings that do not share by default — the distances and the
driver-level toggles a weaker machine has to be free to disagree about.

**Rejected:** a table of every `options.txt` key for every version. It is the
thorough answer and it is a maintenance subscription — every release, for every
key, forever. Only a setting that actually changed spelling or encoding earns a
versioned entry; everything else is a direct key that needs no translation, and
the cost of a missing entry is a setting that stops being shared rather than one
that corrupts.
