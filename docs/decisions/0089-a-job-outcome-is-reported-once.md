# A job outcome is reported once, by the job store

*Applies to: [Front-ends](../architecture/frontends.md)*

A failed job was reported by up to three handlers: the call site's `mutate`
callback, the mutation cache's generic failure handler, and the job store that
already sees every settle. Each toasted, and each ran its own side effects, so
one failure could surface twice and a dedupe by toast id would only hide that.
Nothing written said which one owns the outcome.

**The job store owns a job's outcome.** The rule is one owner per kind of work:

- The job store notifies a job's success or failure. The mutation cache notifies
  a plain mutation's failure and skips job mutations.
- A call site never toasts a failure.
- A cancel never toasts: the user asked for it.
- A job started by another client (the CLI, a schedule) never toasts; it is
  recorded quietly in the notification list.

**Each surface has one job:**

| Surface | For |
|---|---|
| Toast | the moment an outcome arrives that happened off-screen or in the background; errors and actionable ones persist until dismissed |
| Inline | feedback on the control in view, such as a routine "saved" |
| Notification list | every notice of the session, toasted or not, and anything to come back to |
| `WarningNotice` | a degraded state that is still true |

Every toast is also written to the notification list behind the status bar's
bell, as JetBrains IDEs do with their balloons, so a toast that expired or was
dismissed is never the only record of what happened.

**Rejected:** deduping by toast id alone, because it hides the ownership bug and
still runs the side effects twice; keeping per-call `mutate` callbacks, because
they are the second owner this record removes; and a separate history of
settled jobs beside the notifications, because it showed the same outcome twice
in two shapes.
