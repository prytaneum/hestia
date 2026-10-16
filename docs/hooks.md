# Frontend hooks — the queries layer

The usage guide for `frontend/src/queries/`, the TanStack Query layer the desktop UI is built on. A factory exists
when a screen uses it, so building a page is only
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
or one that accumulates live events. Each is an exported `use*` function in its `queries/<domain>.ts`; everything else
is a factory.

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
const { data: mods } = useQuery(entryContentQueries.list('server', id, 'mod'));
const { data: entries } = useQuery(serverQueries.config(id));
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
`serverQueries.logs(...)`. While following, the tail query stops refetching (the stream
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

`tryCall`-backed reads (`configQueries.value`, …) already surface a missing value as `null` data
rather than an error. Queries and mutations don't retry (the daemon is a local socket — failures aren't transient
network blips), so an error is real the first time you see it. Nor does an errored query refetch when a component
remounts (`retryOnMount:
false`): it clears when something invalidates it — a mutation, a daemon topic, or the reconnect sweep. A daemon that
goes away is not a per-query concern at all; the shell's `OfflineOverlay` reports it once, from `useConnection()`.

## Finding a factory

The query modules are the inventory: each `frontend/src/queries/<domain>.ts` is an object literal of named factories
(`<domain>Queries`, `<domain>Mutations`) plus the few named hooks, and a job mutation is marked by `jobMutation(...)`.
A daemon channel with no factory there is one no screen uses yet.

## Extending the layer

A new channel a screen needs is one factory entry in the domain's `queries/<domain>.ts`. Add a named hook only when a
factory alone cannot express it — a read composing two sources, or one accumulating live events. The full recipe,
including the `mutation()`/`jobMutation()` helpers and the invalidation map, is in
[contributing.md](contributing.md#add-a-desktop-api-method).

