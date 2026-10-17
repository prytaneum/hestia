/**
 * Daemon events as a React hook: subscribe to one topic for the component's
 * lifetime. The handler rides a ref, so an inline closure never re-subscribes.
 */
import { useEffect, useRef } from 'react';
import { onTopic, type Topics } from '../api';

export function useDaemonEvent<K extends keyof Topics>(
  topic: K,
  handler: (payload: Topics[K]) => void,
): void {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    let disposed = false;
    let off: (() => void) | undefined;
    onTopic(topic, (payload) => handlerRef.current(payload))
      .then((unsubscribe) => {
        if (disposed) unsubscribe();
        else off = unsubscribe;
      })
      .catch(() => {
        // Outside the Tauri shell there are no daemon events to hear.
      });
    return () => {
      disposed = true;
      off?.();
    };
  }, [topic]);
}
