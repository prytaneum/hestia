# One network vetting policy serves Hestia and its plugins

*Applies to: [The plugin kernel](../architecture/plugins.md#network-vetting)*

**Decision.** `kernel::vetting` is the only place a request or download is judged: https only, no IP literal or URL
credentials, a granted host (a `*.domain` wildcard matches one label and never a public suffix), a resolver that refuses
addresses inside the machine, time and size caps, and a checksum or a declared unhashed host, all rechecked on every
redirect. The engine's core callers and every plugin go through it.

**Why.** One place enforces the rules, so core and plugins cannot drift apart, and a fix to the policy fixes both.
Wildcards let a plugin name a service's CDN; the public-suffix check (`psl` crate) keeps `*.com` from granting everyone.

**Rejected.** A second policy for plugins only: the two would diverge. Exact host names only: a plugin would have to
list every host of a CDN. A hand-kept suffix list: it misses the private section of the public suffix list.
