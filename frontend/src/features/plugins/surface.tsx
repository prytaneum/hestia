import { useEffect, useState } from 'react';
import type { PluginLink, PluginNotice, PluginQuestion } from '@/api';
import { plugin, system } from '@/api';
import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { Button } from '@/components/ui/button';
import { notify } from '@/lib/notify';
import { m } from '@/paraglide/messages.js';
import { useConnection, useDaemonEvent } from '@/queries';

const NOTICE = {
  info: notify.info,
  warn: notify.warning,
  error: notify.error,
} satisfies Record<PluginNotice['level'], typeof notify.info>;

/**
 * Everything a plugin puts in front of the user. The shell draws it and names
 * the plugin in the title; the plugin supplies only the text, so it cannot pass
 * itself off as Hestia.
 */
export function PluginSurface() {
  const [questions, setQuestions] = useState<PluginQuestion[]>([]);
  const [links, setLinks] = useState<PluginLink[]>([]);

  const connection = useConnection();
  useEffect(() => {
    if (connection === 'disconnected') {
      setQuestions([]);
      setLinks([]);
    }
  }, [connection]);

  useDaemonEvent('plugin.notice', ({ plugin: name, level, message }) =>
    (NOTICE[level] ?? notify.info)(name, { description: message }),
  );
  useDaemonEvent('plugin.question', (question) =>
    setQuestions((queue) => [...queue, question]),
  );
  useDaemonEvent('plugin.question.closed', ({ request }) =>
    setQuestions((queue) => queue.filter((q) => q.request !== request)),
  );
  useDaemonEvent('plugin.link', (link) =>
    setLinks((queue) => [...queue, link]),
  );

  const question = questions[0];
  const link = links[0];

  const settle = (accepted: boolean) => {
    if (!question) return;
    setQuestions((queue) => queue.slice(1));
    void plugin.answer(question.request, question.plugin, accepted);
  };

  const close = (open: boolean) => {
    if (!open) setLinks((queue) => queue.slice(1));
  };

  return (
    <>
      <AppDialog
        open={Boolean(question)}
        onOpenChange={(open) => !open && settle(false)}
        size="sm"
        title={m['plugin.question.title']({ plugin: question?.plugin ?? '' })}
      >
        <DialogBody>
          <p className="text-sm whitespace-pre-wrap">{question?.message}</p>
        </DialogBody>
        <DialogFooter>
          <Button variant="outline" onClick={() => settle(false)}>
            {m['plugin.question.decline']()}
          </Button>
          <Button onClick={() => settle(true)}>
            {m['plugin.question.accept']()}
          </Button>
        </DialogFooter>
      </AppDialog>
      <AppDialog
        open={Boolean(link)}
        onOpenChange={close}
        size="sm"
        title={m['plugin.link.title']({ plugin: link?.plugin ?? '' })}
        description={m['plugin.link.body']()}
      >
        <DialogBody>
          <p className="text-sm font-mono break-all">{link?.url}</p>
        </DialogBody>
        <DialogFooter>
          <Button variant="outline" onClick={() => close(false)}>
            {m['plugin.link.dismiss']()}
          </Button>
          <Button
            onClick={() => {
              if (link) void system.openUrl(link.url);
              close(false);
            }}
          >
            {m['plugin.link.open']()}
          </Button>
        </DialogFooter>
      </AppDialog>
    </>
  );
}
