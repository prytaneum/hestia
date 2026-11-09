# Bedrock hot backups follow `save hold`, and copy what the server reports

*Applies to: [Servers & instances](../architecture/entries.md); extends [0021](0021-backups-follow-docker-mc-backup.md)*

A Java backup pauses saving with `save-off` and `save-all flush` and archives
`data/` ([0021](0021-backups-follow-docker-mc-backup.md)). A Bedrock world is
LevelDB, which keeps writing files while the server is up: copying them as they
stand can tear the database.

**A live Bedrock server is backed up by BDS's documented protocol.** The backup
sends `save hold`, polls `save query` until the server reports the world files
are ready (every 500 ms, giving up after 30 s), and the reply lists each file
with the length that is consistent. The archive copies each listed file
**truncated to that length**. `save resume` then runs whatever the archive's
outcome, as `save-on` does for Java. A server that never becomes ready is a
failed backup, not a silently torn one.

The edition supplies the pause, the resume command and the truncation table; the
archive step is shared, and an empty table means "copy whole".

**Rejected.** Copying without truncation, which can produce a world that does not
load. Stop, copy, start, which costs downtime.
