# Architecture decisions

## ADR 001 — local authority

Accepted: Tauri 2 + React/TypeScript UI; Rust core owns SQLite. No hosted API, cloud database, account or telemetry. This follows PRD v1.2, not the superseded cloud-first conversation. A native IPC boundary avoids running a localhost application API.

## ADR 002 — shared domain objects

This slice uses a constrained `Item` union for notes/tasks/saved discoveries. Workspace ownership currently means the single local personal workspace; multiple workspaces and cross-entity relations require a later migration. Do not claim multi-tenant isolation. Future domain modules reuse capture/search and explicit relationship tables rather than making independent note systems.

## ADR 003 — sync-compatible, not sync-complete

UUIDs, device provenance, revision checks, transactional change events, tombstone fields and cursor storage are present. No transport or peer pairing exists. The journal records full item payloads; operations are `upsert`/`delete`, with event ID, local sequence, entity, revision, time and device. Sequence numbers are local, not globally ordered. Current import preserves entity provenance and writes local journal entries; peer protocol must define imported-event semantics and conflict handling before shipping.

## ADR 004 — portable snapshots

Export snapshots are independently readable. Native dialogs choose paths; renderer cannot supply arbitrary paths. Import validates all entries before transaction and skips existing IDs; resolving conflicts by replacement is deferred. Export is not sync and is not yet complete database-history backup. Password encryption has fixed versioned parameters and does not export OS secrets.

## ADR 005 — explicit providers

`domain.ts` defines `LocalRepository`, `ExternalSourceConnector`, `AIProvider`, `SyncTransport` and `BackupProvider` contracts. These are extension seams, not claims that adapters exist. Future network access must be opt-in, use backend allowlists, timeouts and scope-aware data policies. Sensitive records require a separately encrypted store before their domain UI is enabled.

## ADR 006 — deterministic first command palette

Keyword search and explicit create/navigation actions are useful without an LLM. No natural-language interpretation is falsely advertised. Local AI, voice, OS global shortcuts and semantic retrieval will extend this foundation.

## ADR 007 — v0.2 workspace and device separation

SQLite schema 2 adds structured records with FTS, links, status and device provenance; schema 3 adds a disposable embedding cache. The ordinary workspace remains single-user/single-personal-workspace; domain tabs are not separate tenancy boundaries. Device-local folder bindings, OS keys and caches are excluded from portable records.

The original ADR 004 describes archive schema 1. Schema 2 now includes ordinary domain records and event history; explicitly selected vault data is included only under password encryption. Atomic export writes and persistent verification metadata replace the original direct-write limitation. Restore can keep local conflicts or import separate copies with remapped IDs; no destructive overwrite mode exists.

## ADR 008 — native integrations without a hosted backend

Fixed-source HTTPS discovery and loopback Ollama run through the Rust bridge. Network operations require explicit frontend consent parameters and have bounds/timeouts. Read-only Git observation uses only native-selected roots. Native OS notification and global shortcut plugins expose narrow app behaviors. No model-driven shell, MCP execution or external send capability is present.

## ADR 009 — sensitive vault

A random per-device AES-256 key lives in OS secure storage. Vault record JSON, including document bytes and filenames, is encrypted in SQLite with UUID-bound authenticated data. Private backup export uses a distinct user-password envelope; restore rekeys the data on the destination. Vault sessions are deliberately separate from generic search/AI, and no vault context reaches models.

## ADR 010 — independently useful mobile companions

SwiftUI and Compose companions own separate SQLite stores. They share an ordinary-item JSON exchange contract, not a live backend or premature sync protocol. Voice is on-device only. Mobile feature breadth and V2 authenticated LAN synchronization remain separate unfinished milestones.

## V2 shared replication core (v0.4)

Storage schema 4 adds causal heads/events, conflict history and peer cursors. `crates/mobile-peer` exposes the same repository and Noise protocol to Swift C ABI and Kotlin JNI; its optional receiver has explicit foreground lifecycle controls. Desktop peer state lives in Tauri; OS secure storage owns device keys and pinned trust. Private category transfer remains separate from the ordinary journal. See [V2 design](V2-SYNC-DESIGN.md) for protocol bounds, trust, recovery, notification receipts and backup behavior.
