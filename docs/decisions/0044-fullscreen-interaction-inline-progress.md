# Interaction is fullscreen; bare progress is one line

*Applies to: [Front-ends](../architecture/frontends.md)*

The inline ratatui viewport (a fixed-height strip above the cursor) could not
follow terminal resizes and left every widget fighting for rows, so it is gone:
every widget (anything that draws a viewport and takes keys) owns the whole
alternate screen for exactly as long as it runs, then hands the shell back
intact. Line prompts and streams are not widgets: a `read_line` question
(`ui::input`, `ui::confirm`) and a stream of lines (the `logs -f` follower, the
attach after a launch) draw no viewport, so they print to the scrollback where
the terminal's own scrolling, search and copy work on them. The deliberate
exception to the alternate screen is progress with no interaction
(`java install`, `backup create`, a detached start): flashing the alternate screen for a spinner the user cannot act on is hostile,
so the `Spinner`/reporter API renders one stderr line rewritten in place (and
terse per-phase lines when redirected). Progress that happens *inside* a flow —
installing a reviewed content batch, provisioning from the create wizard —
renders in-session on the same screen that collected the decision.
