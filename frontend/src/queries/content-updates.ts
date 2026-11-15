/**
 * Tells the user when a daemon check finds content updates nothing had
 * reported before — at startup, on reconnect, as an instance launches, or
 * when another client opened an entry. The notice lands in the bell, and the
 * library and content lists mark what can move.
 */
import { onDaemonEvent } from '../api';
import { notify } from '../lib/notify';
import { m } from '../paraglide/messages.js';

const NOTICE_ID = 'content-updates-found';

let started = false;

export function startUpdateNotices(): void {
  if (started) return;
  started = true;
  onDaemonEvent((event) => {
    if (event.topic !== 'content.updates_found') return;
    notify.info(
      m['app.toast.content_updates_found']({ count: event.payload.fresh }),
      { id: NOTICE_ID },
    );
  }).catch(() => {
    // Outside the Tauri shell there are no daemon events to hear.
  });
}
