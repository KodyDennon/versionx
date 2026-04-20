---
title: State DB schema
description: The local SQLite database Versionx uses as a cache. Tables, semantics, and the safe-to-delete contract.
sidebar_position: 5
---

# State DB schema

Versionx keeps a local SQLite database at `$XDG_DATA_HOME/versionx/state.db` (or platform equivalent). It is **strictly a cache** — nothing in it is load-bearing for correctness. If you delete it, the next `versionx sync` rebuilds it from git, config, and lockfile.

## The safe-to-delete contract

> Nothing in the state DB may be load-bearing for correctness.

This is principle 4 in the [design principles](/introduction/design-principles). It means:

- Deleting `state.db` is always safe.
- Versionx never refuses an operation because the DB is missing.
- The DB is never the source of truth for any committable artifact.

What the DB is used for:

- TUI and daemon performance (avoid re-parsing every `versionx.toml` on every invocation).
- Cross-repo queries (the Dashboard view needs to know about every tracked repo).
- Audit trail of past runs (for debugging, not for enforcement).

## Driver

- `rusqlite` with the `bundled` feature (no system SQLite dependency).
- WAL mode.
- Migrations via `rusqlite_migration`.

## Tables

The schema evolves; current Stable tables in 0.7:

### `workspaces`

Every repo Versionx has seen.

```sql
CREATE TABLE workspaces (
  id            INTEGER PRIMARY KEY,
  path          TEXT NOT NULL UNIQUE,
  name          TEXT NOT NULL,
  topology      TEXT NOT NULL,
  first_seen    TIMESTAMP NOT NULL,
  last_seen     TIMESTAMP NOT NULL,
  config_blake3 TEXT NOT NULL
);
```

### `runs`

Audit trail of every `plan` / `apply`.

```sql
CREATE TABLE runs (
  id             INTEGER PRIMARY KEY,
  workspace_id   INTEGER NOT NULL REFERENCES workspaces(id),
  kind           TEXT NOT NULL,           -- sync / update / release / install
  started_at     TIMESTAMP NOT NULL,
  ended_at       TIMESTAMP,
  outcome        TEXT,                    -- ok / err / cancelled
  plan_blake3    TEXT,
  events_zstd    BLOB                     -- compressed event stream
);
```

### `runtimes_installed`

Inventory of cached runtimes.

```sql
CREATE TABLE runtimes_installed (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL,            -- node / python / rust / ...
  version       TEXT NOT NULL,
  path          TEXT NOT NULL,
  installed_at  TIMESTAMP NOT NULL,
  sha256        TEXT NOT NULL,
  UNIQUE (kind, version)
);
```

### `policy_waivers`

Mirror of `.versionx/waivers.toml` plus fleet-inherited waivers, indexed for fast lookup.

```sql
CREATE TABLE policy_waivers (
  id            INTEGER PRIMARY KEY,
  workspace_id  INTEGER NOT NULL REFERENCES workspaces(id),
  rule_id       TEXT NOT NULL,
  expires       TIMESTAMP NOT NULL,
  granted_by    TEXT NOT NULL,
  reason        TEXT NOT NULL
);
```

### `sagas`

Running and completed multi-repo sagas.

```sql
CREATE TABLE sagas (
  id            TEXT PRIMARY KEY,         -- UUIDv7
  started_at    TIMESTAMP NOT NULL,
  ended_at      TIMESTAMP,
  state         TEXT NOT NULL,            -- planning / applying / compensating / done / failed
  plan_blake3   TEXT NOT NULL,
  steps_json    TEXT NOT NULL             -- serialized step list
);
```

## Inspecting

```bash
versionx state inspect                    # summary
versionx state inspect workspaces         # dump a specific table
versionx state sql "SELECT * FROM runs LIMIT 5"   # ad-hoc SQL (read-only by default)
```

## Rebuilding

```bash
versionx state rebuild
```

Drops every table, walks known workspaces from git and config, repopulates. Takes a few seconds on most machines.

## Remote state (post-1.0)

The same schema ports to Postgres for fleet deployments. See [Roadmap](/roadmap) — remote state lands in 1.2.

## See also

- [Events & tracing](./events) — the event stream that feeds the `runs` table.
- [Environment variables](./environment-variables) — `VERSIONX_DATA_HOME` to override the DB location.
