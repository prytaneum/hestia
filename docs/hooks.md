# Frontend hooks — the queries layer

The usage guide for `frontend/src/queries/`, the TanStack Query layer the desktop UI is built on. It mirrors the typed
API (`frontend/src/api/`) **1:1** — every channel the daemon serves has a factory — so building a page is only
rendering: no `call()`, no event-bus wiring, no cache bookkeeping in components.
[contributing.md](contributing.md#add-a-desktop-api-method) has the recipe for
*extending* the layer; this page is how to *consume* it.

```
frontend/src/
├── api/          typed channel calls (no caching) — mirrors the Rust client SDK
└── queries/      this layer: factories + hooks + jobs + invalidation
    ├── client.ts        the QueryClient singleton + invalidate()
    ├── keys.ts          the hierarchical query-key factory
    ├── core.ts          mutation() — the plain-mutation factory helper
    ├── jobs.ts          the global job store + jobMutation()/useJobMutation
    ├── invalidation.ts  daemon topics → key prefixes (installed at bootstrap)
    ├── connection.ts    useConnection()
    ├── events.ts        useDaemonEvent(topic, handler)
    ├── logs.ts          log following shared by the per-domain log hooks
    └── <domain>.ts      server, instance, java, … — the query/mutation factories
```

Import everything from the barrel:

```ts
import { serverQueries, serverMutations, useServer, keys } from '#/queries';
```

## The mental model

Each domain module exports **factories** — options objects, not hooks. You pass them to TanStack's own
`useQuery`/`useMutation`, which keeps every call site explicit about what it is doing and lets the same definition serve
a component, a route loader and an imperative fetch:

- **`<domain>Queries`** — `queryOptions` factories. The single definition of a read: its key, its fetch, its staleness.
  Usable anywhere an options object is: `useQuery(serverQueries.detail(id))`, a router loader's
  `ensureQueryData`, `queryClient.fetchQuery`.
- **`<domain>Mutations`** — mutation-options factories. The single definition of a write: what it calls and which key
  prefixes it invalidates on settle (declared as data, not scattered in components).
```tsx
const { data: servers } = useQuery(serverQueries.list());
const start = useMutation(serverMutations.startAny());
const create = useJobMutation(serverMutations.create());
```

A **handful of named hooks** exist on top, only where a factory alone is not enough — a read that composes two sources,
or one that accumulates live events. They are listed in the inventory below; everything else is a factory.

Rules that hold everywhere:

- **Per-entry hooks take the stable `id`** (from list data), never the display name. The wire resolves either, but a
  rename must not strand a cache key or a mutation key.
- **Errors are `HestiaError`** (`error.code` is the daemon's error-code vocabulary — `not_found`, `bad_request`, …). The
  type is registered globally, so `query.error`/`mutation.error` are typed without casts.
- **Freshness is automatic.** Mutations invalidate their own prefixes on settle, and the daemon-event feed
  (`invalidation.ts`) sweeps keys when the CLI, the tray, or the scheduler changes something. Components never call
  `invalidateQueries` themselves.

## Reads

```tsx
function ServerList() {
    const { data: servers, isPending, error } = useQuery(serverQueries.list());
    if (isPending) return <Spinner />;
    if (error) return <p role="alert">{ error.message }</p>;
    return servers.map((s) => <ServerCard key={ s.id } id={ s.id } />);
}
```

`useServer(id)` seeds from the list cache, so rendering a row of an already-fetched list costs no extra call; the status
query then keeps it fresh. Instances have no status channel, so `useInstance(id)` selects the entry out of the list
query — same shape to the caller.

```tsx
const { data: server } = useServer(id);                              // ServerInfo | undefined
const { data: backups } = useQuery(serverQueries.backups(id));
const { data: mods } = useQuery(serverQueries.content(id, 'mod'));
const { data: value } = useQuery(serverQueries.configValue(id, 'memory'));   // string | null
```

Catalogue reads (`serverQueries.flavors()`, `serverQueries.versions(flavor)`,
`javaQueries.releases()`, content browse) carry a longer `staleTime` — upstream catalogues don't change mid-session.

## Loading skeletons

Pending reads render hand-drawn, theme-matched skeletons — ordinary components over the same tokens (`bg-muted`, the
app's square corners), no capture step and no dependency. `Bone` (`components/skeleton.tsx`) is the pulsing primitive;
`CardGridSkeleton` mirrors a page's real grid classes so bones land where cards will; a page-shaped composition lives
beside any page that needs more (`features/skins/components/skeleton.tsx`, the entry grid shared by
servers/instances/library in `features/shared/entry/components/skeleton.tsx`). Two seams:

- A routed page passes `skeleton={<… />}` with its pending flag to `Page` —
  `<Page skeleton={<EntryGridSkeleton />} loading={isPending} …>` — and the body swaps for the skeleton while the header
  stays live.
- A surface outside `Page` (the sidebar account row) renders its bones inline while its query is pending.

## Writes

Every mutation factory yields a standard TanStack mutation: `mutate` /
`mutateAsync`, `isPending`, `error`, `data`. The variables are whatever the verb needs *beyond* the entry already bound
at hook time:

```tsx
const start = useMutation(serverMutations.start(id));
const rename = useMutation(serverMutations.rename(id));
const setConfig = useMutation(serverMutations.setConfig(id));

start.mutate();                                  // no variables
rename.mutate('Cozy SMP');                       // the new name
setConfig.mutate({ key: 'memory', value: '4G' });
```

```tsx
<Button onClick={ () => start.mutate() } disabled={ start.isPending }>
    Start
</Button>
{
    start.error && <p role="alert">{ start.error.message }</p>
}
```

Invalidation happens on settle via the factory's `invalidates` — after
`start` settles, the server lists/details and process queries refetch on their own. `mutate` accepts the usual per-call
callbacks when a component needs them (`onSuccess: (server) => navigate(…)`).

## Long-running jobs

Anything that streams progress events — server create/update, instance launch, backups, content installs, java installs,
downloads — is a **job mutation**. Two guarantees:

1. **Every run lands in the global job store**, no matter which component fired it or whether that component is still
   mounted. An activity panel/toast surface subscribes with `useJobs()`; a per-entry busy indicator with
   `useEntryJobs(kind, id)`.
2. **`useJobMutation` adds the local view**: the same mutation result plus
   `progress` (the job's latest progress payload) and `job` (the store's record) for the run this call site started.

```tsx
function CreateServer() {
    const create = useJobMutation(serverMutations.create());

    const submit = (form: ServerCreateParams) =>
        create.mutate(form, {
            onSuccess: (server) => navigate({ to: `/servers/${ server.id }` }),
        });

    if (create.isPending)
        return (
            <ProgressBar
                label={ create.progress?.phase }          // 'resolving' | 'java' | …
                value={ create.progress?.current }
                max={ create.progress?.total }
            />
        );
    return <WizardForm onSubmit={ submit } error={ create.error } />;
}
```

```tsx
function ActivityPanel() {
    const jobs = useJobs();
    return jobs.map((job) => (
        <Row key={ job.id }>
            <span>{ job.label }</span>
            { job.status === 'running' && <Gauge progress={ job.progress } /> }
            { job.status === 'error' && <em>{ job.error?.message }</em> }
            { job.status !== 'running' && (
                <button type="button" onClick={ () => dismissJob(job.id) }>×</button>
            ) }
        </Row>
    ));
}
```

A `Job` carries `kind` (`'server.create'`, `'backup.restore'`, …), an optional `entry`
(`{ kind: 'server' | 'instance', id }`), `status`
(`running | done | error`), the latest `progress`, and timestamps. Settled jobs stay listed until `dismissJob(id)` /
`clearSettledJobs()` (the store caps how many settled jobs it keeps).

A component that fires a job but renders no inline progress can use plain
`useMutation(serverMutations.backup.create(id))` — the run is tracked globally either way; `useJobMutation` only adds
the local `progress` view.

**A job is on screen once.** The status bar carries the *backgrounded* jobs, so a dialog that renders progress claims
its job with `useJobDisplay(job, shown)` for as long as it shows it. Dismissing the dialog (or unmounting it) releases
the claim, which is what hands the job to the status bar:

```tsx
const create = useJobMutation(serverMutations.create());
useJobDisplay(create.job, open && create.isPending);
```

## Live data

**Connection state** — the shell's watcher, as a hook. On reconnect the layer invalidates every query itself; the banner
is all the UI owes:

```tsx
const connection = useConnection();   // 'connected' | 'disconnected'
if (connection === 'disconnected') return <Banner>Daemon unreachable…</Banner>;
```

**Logs** — the fetched tail plus `process.output` events accumulated on top. `lines` is the merged view; the rest is the
underlying query result:

```tsx
function Console({ id }: { id: string }) {
    const logs = useServerLogs(id, { tail: 200, follow: true });
    const command = useMutation(serverMutations.command(id));
    return (
        <>
            <LogView lines={ logs.lines } />
            <Input onSubmit={ (line) => command.mutate(line) } />
        </>
    );
}
```

`useInstanceLogs(id, { session, follow })` follows one named session or every session of the instance;
`useFollowedLogs(query, matches, limit)` is the primitive both compose over, usable directly against
`processQueries.logs(...)`. While following, the tail query stops refetching (the stream
is the freshness) and the live buffer is capped by `limit`
(default 1000).

Following is scoped to the **entry**, not to one run of it, so leave
`follow` on while the entry is stopped: the stream resumes on the next start with no remount and no manual refresh. A
`process.started` for the entry resets the live buffer, since the new run writes a fresh log — the refetched tail is
that run's history.

**Raw events** — for the rare component that needs a daemon topic directly (the payload shapes mirror `crates/proto`'s
`Topic` structs):

```tsx
useDaemonEvent<ProcessExit>('process.exit', (exit) => {
    if (!exit.success) toast.error(`${ exit.id } crashed`);
});
```

## Route loaders

Factories plug straight into TanStack Router. The root route context carries the `queryClient`:

```tsx
export const Route = createFileRoute('/servers/')({
    loader: ({ context }) =>
        context.queryClient.ensureQueryData(serverQueries.list()),
    component: ServersPage,   // the same factory renders instantly from the cache
});
```

## Error handling

```tsx
const { error } = useServer(id);
if (error && isNotFound(error)) return <NotFound />;   // from '#/api'
```

`tryCall`-backed reads (`configQueries.value`, `serverQueries.configValue`, …) already surface a missing value as `null` data
rather than an error. Queries and mutations don't retry (the daemon is a local socket — failures aren't transient
network blips), so an error is real the first time you see it. Nor does an errored query refetch when a component
remounts (`retryOnMount:
false`): it clears when something invalidates it — a mutation, a daemon topic, or the reconnect sweep. A daemon that
goes away is not a per-query concern at all; the shell's `OfflineOverlay` reports it once, from `useConnection()`.

## Inventory

The 1:1 audit: every factory, its wire surface, and its variables. *(job)* marks a `jobMutation` — pair it with
`useJobMutation` for the local progress view, or plain `useMutation` to fire and forget.

### Hooks

The only named hooks. Everything else is a factory passed to `useQuery`/`useMutation`/`useJobMutation`.

| Hook                                                    | What                                                        |
|---------------------------------------------------------|--------------------------------------------------------------|
| `useConnection()`                                       | daemon connection state                                       |
| `useDaemonEvent(topic, handler)`                        | one daemon topic, for the component's lifetime                |
| `useJobs()` / `useJob(id)` / `useEntryJobs(kind, id)`   | the global job store                                          |
| `useJobMutation(factory)`                               | a job mutation plus its local `progress`/`job`                |
| `useJobDisplay(job, shown)`                             | claim a job's display while a dialog renders its progress     |
| `dismissJob(id)` / `clearSettledJobs()` / `getJobs()`   | store maintenance (not hooks)                                 |
| `useServers()` / `useServer(id)`                        | the list, and one server seeded from it                       |
| `useInstances()` / `useInstance(id)`                    | the list, and one instance selected out of it                 |
| `useDaemon()`                                           | status gated on the connection, with a live-ticking uptime    |
| `useAccounts()`                                         | the list bundled with its sign-in, switch and remove mutations |
| `useServerLogs(id, opts)` / `useInstanceLogs(id, opts)` | the fetched tail plus live `process.output`                   |
| `useProcessLogs(id, opts)`                              | the raw per-process form of the same                          |
| `useFollowedLogs(query, matches, limit)`                | the follow primitive those three compose over                 |
| `useProcessMetrics(processId, window)`                  | `process.metrics` samples over a rolling window               |
| `useEquippedSkin(account?, opts)`                       | the account's current skin, selected out of `skin.list`       |
| `useConsoleHistory(id)`                                 | per-server command echoes and RCON replies (in-memory)        |
| `useModpack(kind, id)`                                  | an entry's installed pack                                     |
| `useInstallModpack(kind)` / `useUpdateModpack(kind, id)` *(job)* | the pack jobs                                        |
| `useRemoveModpack(kind, id)`                            | remove the pack record                                        |
| `useUpdateCheck(enabled)` / `useDownloadUpdate()` / `useApplyUpdate()` | self-update, over the daemon's `update.*` channels |
| `usePrefs()` / `usePinned()`                            | desktop-local UI state, over the `prefs_*` shell commands     |
| `useMultiSession()`                                     | whether `instance.multi-session` allows concurrent sessions   |
| `useEntryIconLookup()`                                  | `(id) => iconUrl`, over the `icons_*` shell commands          |

### Servers — `serverQueries` / `serverMutations`

| Factory                                              | Channel                             | Variables                                        |
|------------------------------------------------------|-------------------------------------|--------------------------------------------------|
| `.list()`                                            | `server.list`                       | —                                                |
| `.detail(id)` / `.info(id)`                          | `server.status` / `server.info`     | —                                                |
| `.ping(id)`                                          | `server.ping`                       | —                                                |
| `.flavors()` / `.loaders(flavor)` / `.versions(flavor)` / `.profile(params)` | `server.flavors\|loaders\|versions\|resolve` | —                        |
| `.logs(id, opts)`                                    | `server.logs`                       | —                                                |
| `.config(id)` / `.configValue(id, key)`              | `server.config.list\|get`           | —                                                |
| `.backups(id)`                                       | `server.backup.list`                | —                                                |
| `.content(id, kind)` / `.contentUpdates(id)`         | `server.content.list\|check_updates`| —                                                |
| `.create()` *(job)*                                  | `server.create`                     | `Partial<ServerCreateParams>`                    |
| `.update(id)` *(job)*                                | `server.update`                     | `{ version, loaderVersion?, allowDowngrade? }`   |
| `.rename(id)` / `.remove(id)`                        | `server.rename\|remove`             | `name: string` / —                               |
| `.start(id)` / `.stop(id)`                           | `server.start\|stop`                | —                                                |
| `.restart(id)`                                       | `server.restart`                    | — (settles once the new process is up)           |
| `.startAny()` / `.stopAny()`                         | `server.start\|stop`                | `id: string` — for a list row                    |
| `.command(id)`                                       | `server.command`                    | `line: string`                                   |
| `.setConfig(id)`                                     | `server.config.set`                 | `{ key, value }`                                 |
| `.backup.create(id)` *(job)*                         | `server.backup.create`              | —                                                |
| `.backup.restore(id)` *(job)*                        | `server.backup.restore`             | `backupId: string`                               |
| `.backup.remove(id)`                                 | `server.backup.remove`              | `backupId: string`                               |
| `.content.*`                                         | see **Entry content** below         |                                                  |

### Instances — `instanceQueries` / `instanceMutations`

| Factory                                              | Channel                               | Variables                                      |
|------------------------------------------------------|---------------------------------------|------------------------------------------------|
| `.list()` / `.info(id)`                              | `instance.list\|info`                 | —                                              |
| `.flavors()` / `.loaders(flavor)` / `.versions(flavor)` / `.profile(params)` | `instance.flavors\|loaders\|versions\|resolve` | —                  |
| `.worlds(id)`                                        | `instance.worlds`                     | —                                              |
| `.recent()`                                          | `instance.recent`                     | —                                              |
| `.logs(id, opts)`                                    | `instance.logs`                       | —                                              |
| `.config(id)` / `.configValue(id, key)`              | `instance.config.list\|get`           | —                                              |
| `.content(id, kind)` / `.contentUpdates(id)`         | `instance.content.list\|check_updates`| —                                              |
| `.create()`                                          | `instance.create`                     | `Partial<InstanceCreateParams>`                |
| `.update(id)`                                        | `instance.update`                     | `{ version, loaderVersion?, allowDowngrade? }` |
| `.rename(id)` / `.remove(id)`                        | `instance.rename\|remove`             | `name: string` / —                             |
| `.launchAny()` *(job)*                               | `instance.launch`                     | `{ id, newSession?, quickPlay? }`              |
| `.stop(id)` / `.stopAny()`                           | `instance.stop`                       | `{ session? }` / `{ id, session? }`            |
| `.stopSettledAny()`                                  | `instance.stop` + `instance.list`     | `{ id, session }` — settles once it has exited |
| `.setConfig(id)`                                     | `instance.config.set`                 | `{ key, value }`                               |
| `.content.*`                                         | see **Entry content** below           |                                                |

### Entry content — `<domain>Mutations.content` (`entry-content.ts`)

One shared factory set, bound per entry kind, so servers and instances cannot drift.

| Factory                    | Channel                       | Variables                             |
|----------------------------|-------------------------------|---------------------------------------|
| `.add(id)` *(job)*         | `<kind>.content.add`          | `ContentAddInput`                     |
| `.remove(id)`              | `<kind>.content.remove`       | `{ kind, item, worlds? }`             |
| `.update(id)` *(job)*      | `<kind>.content.update`       | `{ kind, item? }` — empty item = all  |
| `.setVersion(id)` *(job)*  | `<kind>.content.set_version`  | `{ kind, item, versionId }`           |
| `.enable(id)`              | `<kind>.content.enable`       | `{ kind, item, enabled }`             |

### Modpacks — `modpackQueries` / `modpackMutations`

`kind` is `'server' | 'instance'`; the channel takes the matching prefix.

| Factory                       | Channel                   | Variables                       |
|-------------------------------|---------------------------|---------------------------------|
| `.status(kind, id)`           | `<kind>.modpack.status`   | —                               |
| `.updateCheck(kind, id)`      | `<kind>.modpack.check_update` | —                           |
| `.install(kind)` *(job)*      | `<kind>.modpack.install`  | `ModpackInstallParams`          |
| `.update(kind, id)` *(job)*   | `<kind>.modpack.update`   | `{ version?, allowDowngrade? }` |
| `.remove(kind, id)`           | `<kind>.modpack.remove`   | `{ keepContent }`               |

### Content browse — `contentQueries`

| Factory                        | Channel                   | Variables                      |
|--------------------------------|---------------------------|--------------------------------|
| `.sources()`                   | `content.sources`         | —                              |
| `.search(query)` / `.searchPaged(query)` | `content.search` | — (paged is infinite-scroll)   |
| `.project(project, source?)`   | `content.project`         | —                              |
| `.versions(query)`             | `content.versions`        | —                              |
| `.url(url)`                    | `content.resolve_url`     | —                              |
| `.modpack(versionId, source?)` | `content.modpack.resolve` | — (heavy — mount deliberately) |

### Everything else

| Factory                                       | Channel                     | Variables                              |
|-----------------------------------------------|-----------------------------|----------------------------------------|
| `appQueries.info()` / `.ping()`               | `app.info` / `health.ping`  | —                                      |
| `daemonQueries.status()`                      | `daemon.status`             | —                                      |
| `daemonMutations.stop()`                      | `daemon.stop`               | `{ stopProcesses }`                    |
| `daemonMutations.start()` / `.restart()`      | `start_daemon` (shell)      | —                                      |
| `configQueries.list()` / `.value(key)`        | `config.list\|get`          | —                                      |
| `configMutations.set()`                       | `config.set`                | `{ key, value }`                       |
| `cacheQueries.info()` / `.list()`             | `cache.info\|list`          | —                                      |
| `cacheMutations.clear()`                      | `cache.clear`               | —                                      |
| `accountQueries.list()`                       | `account.list`              | —                                      |
| `accountMutations.loginSisu()`                | `account_login_sisu` (shell)| —                                      |
| `accountMutations.beginLogin()`               | `account.login.begin`       | `'sisu' \| 'device_code'`              |
| `accountMutations.completeLogin()`            | `account.login.complete`    | `{ id, code? }`                        |
| `accountMutations.switch()` / `.remove()`     | `account.switch\|remove`    | `account: string`                      |
| `javaQueries.releases()` / `.runtimes()`      | `java.releases\|list`       | —                                      |
| `javaMutations.install()` *(job)*             | `java.install`              | `{ major, force? }`                    |
| `javaMutations.uninstall()`                   | `java.uninstall`            | `major: number`                        |
| `processQueries.list()` / `.status(id)` / `.logs(id, opts)` | `process.list\|status\|logs` | —                          |
| `processMutations.start()` / `.stop()`        | `process.start\|stop`       | `ProcessSpec` / `id: string`           |
| `skinQueries.list(account?)`                  | `skin.list`                 | —                                      |
| `skinMutations.add()`                         | `skin.add`                  | `{ account?, name?, variant, data }`   |
| `skinMutations.update()`                      | `skin.update`               | `{ key, label?, variant? }`            |
| `skinMutations.equip()` / `.reset()` / `.remove()` | `skin.equip\|reset\|remove` | `{ key, account? }` / `{ account? }?` / `key` |
| `skinMutations.equipCape()` / `.clearCape()`  | `cape.equip\|clear`         | `{ cape, account? }` / `{ account? }?` |
| `syncQueries.config()` / `.status()` / `.options()` | `sync.get\|status\|options.get` | —                            |
| `syncQueries.sources(unit)`                   | `sync.sources`              | —                                      |
| `syncMutations.enable()` / `.disable()`       | `sync.enable\|disable`      | `{ unit, source? }` / `SyncUnit`       |
| `syncMutations.setUnsynced()`                 | `sync.options.keys`         | `string[]`                             |
| `syncMutations.setOption()`                   | `sync.options.set`          | `{ key, value }`                       |
| `syncMutations.setInstanceUnit(id)`           | `instance.sync.unit`        | `{ unit, shared }`                     |
| `syncMutations.setInstanceUnsynced(id)`       | `instance.sync.keys`        | `string[]`                             |
| `updateQueries.check()`                       | `update_check` (shell)      | —                                      |
| `updateMutations.install()`                   | `update_install` (shell)    | —                                      |
| `iconQueries.list()`                          | `icons_list` (shell)        | —                                      |
| `iconMutations.set()` / `.remove()`           | `icon_set\|remove` (shell)  | `{ entryId, sourcePath }` / `entryId`  |
| `prefsQueries.list()`                         | `prefs_list` (shell)        | —                                      |
| `prefsMutations.set()` / `.remove()`          | `prefs_set\|remove` (shell) | `{ key, value }` / `key`               |
| `downloadMutations.start()` *(job)*           | `download.start`            | `Omit<DownloadSpec, 'id'>`             |

## Extending the layer

A new channel is one factory entry in the domain's `queries/<domain>.ts` (and a row here). Add a named hook only when a
factory alone cannot express it — a read composing two sources, or one accumulating live events. The full recipe,
including the `mutation()`/`jobMutation()` helpers and the invalidation map, is in
[contributing.md](contributing.md#add-a-desktop-api-method).
