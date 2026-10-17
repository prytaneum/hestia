# A temp artifact is only valid while its job holds the claim — so a restart invalidates every one

*Applies to: [The engine](../architecture/engine.md)*

Write-through-a-temp is used all over: the backup archive's `.part`, the
downloader's `.part`, the Java installer's `.staging` tree and its downloaded
archive. The convention assumes the process that created one either finishes or
cleans up — precisely what a crash breaks, and nothing reaped them: killing the
daemon mid-archive correctly refused to promote the partial backup (the rename
*is* the commit, so `backup list` stayed clean) but left
`20260725-093946-manual.tar.gz.part` in `backups/` forever, and the same hole
existed for `.staging`. Stating it as one invariant fixes the class rather than
the file: an artifact is valid only while its job holds the matching in-flight
claim (`Claims`, `runtime/claims.rs`), and no claim survives a restart —
so at startup every artifact still on disk is abandoned by definition.
`engine/reclaim.rs` is that pass, composed by `Engine::reclaim_temp()` and
called from the daemon's boot beside `ProcessSupervisor::recover()` (and again
after a data-home change, since the new home's artifacts are no more claimed
than the old one's). It is deliberately **not** recursive — each subsystem knows
the one directory its artifacts land in, and walking a data home whose asset
store is six figures of files is not something to pay at every start. A backup
additionally reclaims before writing, so the store is tidy whether or not a
restart intervened. The downloader's own `.part` files need no sweep: they sit
at the destination and a retry truncates them, so they are self-healing rather
than accumulating. Reclaimed bytes are logged, so the leak is visible rather
than silent.
