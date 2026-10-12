/**
 * The channel registry — the mock's answer to the daemon's `services/`. One
 * module per domain, merged here; adding a domain is a `import` and a line in
 * the spread, and nothing else in the mock changes.
 */
import type { Handlers } from '../support';
import { channels as account } from './account';
import { channels as app } from './app';
import { channels as cache } from './cache';
import { channels as config } from './config';
import { channels as content } from './content';
import { channels as download } from './download';
import { channels as files } from './files';
import { channels as instance } from './instance';
import { channels as java } from './java';
import { channels as job } from './job';
import { channels as modpack } from './modpack';
import { channels as net } from './net';
import { channels as players } from './players';
import { channels as process } from './process';
import { channels as screenshot } from './screenshot';
import { channels as server } from './server';
import { channels as skin } from './skin';
import { channels as sync } from './sync';
import { channels as tasks } from './tasks';
import { channels as transfer } from './transfer';
import { channels as update } from './update';

export const channels: Handlers = {
  ...app,
  ...account,
  ...cache,
  ...config,
  ...content,
  ...download,
  ...files,
  ...instance,
  ...java,
  ...job,
  ...modpack,
  ...net,
  ...players,
  ...process,
  ...server,
  ...skin,
  ...screenshot,
  ...sync,
  ...tasks,
  ...transfer,
  ...update,
};
