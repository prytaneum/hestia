# Backups follow docker-mc-backup, minus what the launcher already owns

*Applies to: [Servers & instances](../architecture/entries.md)*

The reference behaviour (itzg/docker-mc-backup) is: pause world writes over RCON
(`save-off`, `save-all flush`), tar the data, `save-on` guaranteed by an exit
trap, timestamped `%Y%m%d-%H%M%S` gzip archives, exclude `*.jar,cache,logs`,
prune on a schedule. Hestia keeps that shape and diverges where the launcher
knows more than a sidecar can: excluded binaries (jar, `libraries/`) are
*carried over* on restore rather than missing, because the record's profile —
not the archive — says which version the entry runs; restore is a staged swap
instead of an extract-into-empty-dir script; retention is count-based per kind,
pruning only `scheduled` archives so a deliberate manual or pre-update backup is
never auto-deleted; and the schedule lives on the server record
(a `Backup` task with a retention count, beside its restart and command tasks)
rather than a sidecar's environment. Version updates always back up first — an update is the one moment
data provably changes shape, and the confirmation gate (downgrade warnings)
already marks it as risky. Backups are **server-only**: an instance is an
interactive client session with no RCON channel to quiesce it and no analogue of
a long-running server's unattended schedule, and archive/restore proved the
wrong tool for it — instance **import/export is the intended replacement and is
deferred**, so until it lands instance data has no backup story at all (the
instance `update` downgrade warning states that nothing is backed up).
