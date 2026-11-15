# Personal AI Command Center — Product Requirements Document
Version: 1.2 | Date: 2026-11-14 | Status: Local-only default, user-owned export/backup, peer-first sync; supersedes v1.1 infrastructure decisions

## 1. Vision
Build a customizable, local-first personal life-management and intelligence platform. It continuously discovers relevant information, remembers it, lets the user find anything through natural-language voice/text commands, and helps convert discoveries into tracked projects, actions, and opportunities. Initially single-user, architected for multi-user commercialization later. All functional domains below are in scope; delivery phases only sequence implementation.

## 2. Target platforms and distribution
- Windows: full desktop application, signed .exe installer (MSIX optional later).
- macOS: full desktop application, signed/notarized .dmg distribution.
- iOS and Android: lightweight native companion applications, App Store and Google Play ready.
- Desktop is primary: dashboards, research, agents, integrations, development/project observation, customization.
- Mobile: voice/text AI, quick capture, notes, pinned items, tasks, reminders, notifications, daily briefs, saved content, project status, approval requests.
- Infrastructure: no application-owned cloud backend required in V1. Each device owns its local database, AI/runtime integrations and files. Internet access is used directly for live feeds, external APIs and optional remote AI. V2 adds optional device sync and encrypted cloud backup; Google Cloud is only a possible later host, never a V1 dependency.

## 3. Experience principles
1. Universal AI command bar reachable from any screen; desktop global shortcut; mobile voice-first capture.
2. Every item can be saved, pinned, summarized, tagged, linked to project, converted to task, or investigated.
3. Three intents: Navigate (open exact relevant view), Answer (grounded response with sources), Act (perform approved operation).
4. Fast: instant local search/notes; cached feeds and progressive remote results; explicit loading and stale-data states.
5. Proactive but controllable: configurable daily briefings, thresholds, watchlists, quiet hours, digests, opt-in notifications.
6. User controls visibility, permissions, models, data retention, sources, module enablement, and personalization.
7. All key outputs have provenance, timestamp, source URL, confidence/verification status when applicable.

## 4. Functional domains — all required
### 4.1 Unified home and intelligence feeds
Personal dashboard with daily/weekly briefs, urgent tasks, pinned notes, project activity, discoveries, calendar, alerts, opportunity suggestions. Configurable widgets, interests, filters, read/unread, bookmarks, source ranking, deduplication, relevance and trend-velocity scoring. Feed updates via schedules and on-demand refresh.

### 4.2 Technology discovery
- GitHub: trending repositories, growth velocity, commits, issues, releases, maintainers, languages, adoption signals, watchlists.
- AI/LLM: Hugging Face and other models, new open-source/commercial models, capabilities, licenses, benchmarks, local-hardware compatibility, inference frameworks.
- Emerging technology: research papers, arXiv, patents, news, developer tools, infrastructure, AI agents, robotics breakthroughs, startups, Product Hunt, Hacker News.
- YouTube: trending videos/channels, creator growth signals, educational content, topics for own channel, ideas/scripts/content plans.
- App market: rising iOS/Android apps, categories, feature patterns, UX, monetization and competitor analysis; distinguish estimated metrics from verified metrics.

### 4.3 Robotics Lab
Aerial drones, aquatic ROVs/surface vessels, ground rovers, humanoid/manipulator systems, ROS/simulation, autonomy, hardware components, sensors, BOMs, budget, build difficulty, commercial use cases, project workspaces, tutorials, experiments, and alerts.

### 4.4 Agri-tech Hub
Precision agriculture, drones, irrigation, solar pumps, orchard automation, sensors, farm robotics, crop intelligence, equipment/suppliers, commodity signals, government schemes/subsidies, agricultural expos in India/US/global, seasonal reminders, farm-specific research and projects.

### 4.5 Poker Hub
WSOP/WPT/regional/online events, tournaments, satellites, buy-in, guarantees, game type, venue, dates, registration and schedule alerts, watchlists, travel planning, bankroll/expense tracking, results, hand notes and study assistant. Explicitly label unconfirmed schedules.

### 4.6 Local Business Opportunity & CRM
Search local businesses (Austin first; worldwide configurable), identify weak/no/outdated websites and digital presence, audit accessible public websites, score opportunities, estimate implementation/maintenance effort and potential revenue, save leads and contact history, manage pipeline and follow-ups, draft personalized proposals and website demos, track client work, invoices, recurring revenue, profitability. Respect site terms, privacy, anti-spam and outreach consent; human approval before outreach.

### 4.7 Companies, personal operations, finances
Multi-entity/LLC registry, filing and renewal deadlines, contacts, ownership and documents, business tasks, company-specific views; email inbox intelligence, calendar, personal contacts, finance/account/card metadata, transactions, subscriptions, payments, receipts, budgets, and notifications. Use provider-authorized integrations; financial credentials never stored directly. Read-only by default, write actions require permissions.

### 4.8 Notes, tasks, reminders, memory
Instant voice/text notes, AI-created notes, rich text/images/files/links/code, pins, persistent/evergreen notes, tags, project links, semantic search, summarization. Tasks support priority, critical, pinned, persistent until resolved, recurring indefinitely/custom schedules, deadlines, waiting-on, conditional triggers, someday, checklists, and history. Desktop/push/email/SMS (provider required)/voice notifications; quiet hours, snooze, escalation, delivery history. Personal timeline and contextual recall.

### 4.9 Learning, ideas, content and project incubation
Personal learning paths, courses/tutorials, skill trends, progress and study reminders. Idea capture, feasibility, competition, opportunity scoring, relation graph, idea-to-PRD/prototype pipeline. Creator studio for YouTube topics, scripts and content calendars. Travel/event planning and asset/warranty tracking as optional enabled modules.

### 4.10 MCP and tools marketplace
Discover/refresh MCP registries, official/community servers, versions, capabilities, dependencies, trust and publisher verification, license, install/setup, credentials, permissions, health, update status. Support Unity/Unreal/Blender/game tooling, robotics tools, GitHub, IDEs, databases, cloud, communications, business systems. Support MCP, REST, CLI, native OS APIs and webhooks via a common tool registry. Sandboxed execution, least privilege, explicit user approval for destructive/financial/external communication actions, audit trail.

### 4.11 AI agent and proactive workflows
Model-agnostic assistant with local and cloud models, multi-step research, source-grounded summaries, retrieval over personal content, configurable agent missions, scheduling, automation recipes, cross-domain insight generation, suggestions and notifications. Show agent plans, status, sources, permissions, results, failures and ability to cancel. No unapproved external sends, purchases, deletions or commits.

## 5. Project Intelligence — first-class requirement
### 5.1 Lifecycle
Discovery → Saved → Interested → Exploring → Started → Active → Paused/Blocked → Completed/Archived. Transitions can be made by user command; AI may suggest but not silently assert state.
### 5.2 Project workspace
Name, objective, category, sources, linked GitHub repos, local folders, IDE/workspace, branch, tech stack, AI model(s), tools/MCPs, environment, notes, documents, milestones, tasks, sessions, progress, blockers, next action, timestamps and device provenance.
### 5.3 Local development awareness
Opt-in per folder/repository and device. Desktop agent detects allowed project roots, Git metadata (remote, branch, recent commits, working tree), relevant project manifests, editor workspace metadata and local model/runtime availability. Use filesystem watchers and Git APIs as primary fast path; MCP when needed for richer IDE/tool integrations. Never scan the entire disk or read private files without permission. Never infer code changes solely from an LLM chat session. Clearly label observed vs inferred activity.
### 5.4 Session and device tracking
Record last activity and source device (Windows PC, Mac, mobile), project, repository, timestamp, optional editor and model. Mobile can record notes, planning, approvals and project updates; do not imply it edited local desktop files. Show last session summary, changed files/commits where authorized, next suggested action and resume link. V1 tracks local device sessions; V2 optional sync reconciles changes from other devices with explicit conflict handling.
### 5.5 GitHub integration
OAuth with least-privilege scopes, read repo metadata/commits/issues/PRs; optional write scopes only when explicitly enabled. Link cloud repos to local clones, track progress via actual commits/issues/tasks; support private repos only with permission.
### 5.6 Documents
Attach project files, research PDFs, diagrams, specs, screenshots, and session artifacts; version metadata and indexing. Sensitive documents encrypted; no automatic upload of local project source or files without user opt-in.

## 6. Architecture and technology baseline — local-first, zero mandatory backend
### 6.1 V1 execution model
- Desktop: Tauri 2 + React/TypeScript and Rust native bridge (proposed), packaged as signed Windows .exe installer and signed/notarized macOS .dmg. Full application functionality runs locally; no login or hosted service required for personal data.
- Mobile: native SwiftUI iOS and Kotlin/Jetpack Compose Android companion apps. Each has its own local SQLite database and can capture/view its own content independently. In V1, cross-device state is not promised; import/export can be used as a manual bridge.
- Storage: SQLite is authoritative per device, with FTS5 search, migrations, transactional writes, and an optional local vector index. User-selected local directories hold documents and attachments; sensitive data is encrypted at rest. OS Keychain/Windows Credential Manager protect secrets and encryption key material.
- AI: local inference via user-selected runtimes (e.g. Ollama/llama.cpp), local embeddings/retrieval and local tool orchestration. Optional remote model APIs require user opt-in and explicit data-sending policies.
- Network: application makes direct outbound HTTPS calls to GitHub, YouTube, Hugging Face, news feeds, event sources, approved MCP servers and authorized integrations. API credentials remain local. Refresh jobs run on the user's device, subject to OS background execution and source rate limits; scheduled discovery cannot run while every device is off unless a later hosted scheduler is enabled.
- Agents: local permissioned agent runtime and scheduler, local job history, user-controlled tools and filesystem scopes; no permanent application backend, server database or cloud message queue in V1.
- Offline: previously downloaded content, local notes/tasks/projects, AI models and search remain usable without internet; fresh remote discovery and remote integrations require connectivity.
### 6.2 V2 optional device synchronization — peer-first
- Each device maintains independent local data, a stable device ID, entity IDs, versioned change journal, deletion tombstones and per-device sync cursors. Implement sync-compatible schema in V1, but do not ship a mandatory sync service.
- Preferred same-network path: local peer discovery (mDNS/Bonjour where supported), user-approved pairing (QR code/short verification code), mutually authenticated encrypted channel (TLS 1.3 or equivalent), incremental bidirectional sync. Support desktop-to-desktop and mobile-to-desktop subject to mobile OS networking/background limitations.
- Different networks: evaluate encrypted peer-to-peer connectivity through an established networking stack with NAT traversal, or an optional minimal relay. Direct peer connections across the public internet are not guaranteed; mobile background delivery can require platform push infrastructure. Never expose an unauthenticated listening service to the internet.
- Sync rules: idempotent events, resumable transfers, offline queue, conflict visibility, field-specific merges for simple metadata, explicit conflict resolution for rich notes/documents, and no silent overwrite. Project paths and local binaries are device-specific; sync metadata and activity summaries, not raw source trees or entire local directories by default.
- Pairing, revocation, device loss recovery and end-to-end encryption must be designed before cross-network sync is enabled. Device list displays last successful sync and pending changes.
### 6.3 Portable data export and user-owned backup — V1 baseline, V2 enhancements
- V1 must provide a prominent **Export My Data** workflow without account registration, subscription, or vendor-hosted storage. Support selective export by workspace/module and full export of all user-owned data.
- Provide human-readable CSV (UTF-8, documented headers) for tabular records such as notes metadata, tasks, contacts, projects, and leads; JSON/JSONL for nested structures, relationships, tags, timestamps, history and IDs; Markdown for readable notes; original-format attachments where allowed. Provide a portable ZIP bundle with a manifest, schema version, export timestamp, file hashes, and relative references. Preserve stable IDs and relationship mappings so imports can reconstruct data accurately.
- Offer an optional **encrypted portable archive** for full backups containing private notes, financial metadata, documents and attachments. Use authenticated encryption, strong password-derived keys or a recovery key, and never include plaintext credentials, access tokens, card secrets or OS keychain contents. Plain CSV/JSON exports must show a clear sensitive-data warning and default to excluding highly sensitive fields.
- V1: user chooses a local destination (including an OS-mounted personal cloud folder) and can manually upload the exported file to their own cloud provider. Never upload automatically. Implement **Import / Restore** with validation, preview, duplicate detection, version compatibility, conflict choices and dry-run mode. A successful export must be independently readable outside the app.
- Provide a **Backup & Data Portability** settings screen showing last export, destination, included modules, approximate size, encryption state, and a one-click restore-test/checksum verification action.
- V2: optionally support scheduled encrypted backups to user-selected local drives, NAS, WebDAV, or personally owned cloud storage through provider adapters (e.g., Google Drive, OneDrive, Dropbox, S3-compatible buckets), using user-granted credentials stored locally. Do not require an app-owned server or cloud account.
- Export is not equivalent to continuous synchronization. A portable snapshot cannot safely merge concurrent edits across devices without the separate peer-sync protocol.

### 6.3a Uninstall, reset and data-deletion safeguards
- Provide **Settings → Data & Privacy → Prepare to Uninstall / Reset**: show exactly which databases, attachments, caches, local models and settings are stored on the device; offer Export & Verify, Keep Data, or Delete Local Data. Require explicit confirmation for destructive deletion, with additional confirmation for sensitive data.
- The app must offer a backup reminder during any **app-controlled uninstall/reset flow**, and during an app-controlled Windows uninstaller only where the packaging mechanism supports a reliable interactive hook. macOS users may remove a DMG-installed app by dragging it to Trash; iOS/Android uninstall is OS-controlled. **Do not promise that the app can intercept or display a prompt during every OS-level uninstall.** Instead show backup education during onboarding, in Data & Privacy, before app-controlled resets, and optionally through a configurable periodic reminder.
- Explain that uninstalling an app may or may not remove its user-data directories depending on platform and installer. Never silently wipe user documents or OS-owned data; offer a separate verified **Delete My Local Data** action. Clarify that deleting local data does not erase user-created exports, backups, or data on other devices.
- Before deletion, display the most recent verified backup/export timestamp and a clear **No verified backup found** warning. Deletion remains the user's choice; do not block uninstall or misrepresent that a backup exists.
- Support data portability without proprietary formats or vendor lock-in; document import/export schema and compatibility policy.

### 6.3b LAN synchronization — preferred V2 topology
- **Same Wi-Fi / same LAN is the default V2 sync mode**, with no mandatory cloud service, cloud account, or subscription. Discover peers through mDNS/Bonjour where supported, authenticate through QR or short-code pairing, and sync encrypted incremental changes when both devices are online and reachable.
- Support desktop↔desktop first, then desktop↔mobile, then mobile↔mobile where OS permissions and background limits allow. Include a manual **Sync Now**, connection status, last synced device/time, and queued-change count.
- A Wi-Fi SSID match does not guarantee reachability (guest networks, client isolation, VPNs, firewalls); provide troubleshooting and manual peer address/QR fallback where appropriate.
- Do not sync entire source repositories, local models, caches, integration tokens, or absolute filesystem paths by default. User-selected sensitive documents require explicit per-category sync permission and end-to-end encryption.
- Internet-wide sync is optional and not a launch dependency; if later needed, evaluate direct encrypted P2P, private mesh/VPN, or minimal relay, with clear trade-offs.

### 6.4 Future optional hosted services (V3+ only if needed)
- Support user-triggered or scheduled encrypted snapshots to a user-selected destination (local drive, NAS, or cloud object storage such as Google Cloud Storage), with versioning, integrity checks, restore tests and key recovery guidance.
- Backup is one-way recovery storage and does not by itself merge edits from multiple devices. Cloud backup and cloud relay can be enabled independently. No plaintext sensitive documents or encryption keys should be uploaded together.
- Prefer user-owned cloud storage and minimal server-side metadata; avoid cloud-hosted PostgreSQL, continuously running API servers and ingestion workers unless later features demonstrably need them.
### 6.4 Future optional hosted services (V3+ only if needed)
- For always-on discovery while devices are powered off, reliable internet relay/push, shared multi-user accounts or commercial hosting, add small stateless services behind portable interfaces. Google Cloud is the first optional deployment target; use OCI containers and infrastructure as code so another provider can be substituted.
- Preserve local databases as the source of truth for personal workspaces; hosted components must not become mandatory for basic app use.

## 7. Security, privacy and compliance
- TLS for external API and device-sync transport; secure cookies and CSRF defenses (SameSite, anti-CSRF tokens/origin checks) wherever cookie-authenticated HTTP endpoints exist. Native local calls should use IPC rather than exposing unrestricted HTTP servers. Protect bearer tokens from leakage and the UI from XSS.
- Use OAuth2/OIDC PKCE only for optional third-party account connections; do not require application account sign-in in V1. Secure locally stored integration tokens and support disconnect/revocation. V2 device pairing and revocation must authenticate peers independently of external providers.
- Encrypt sensitive financial records, personal documents, identity files and credentials at rest using per-file or per-record authenticated encryption and locally managed keys protected by OS Keychain/Windows credential facilities; separate policies by data class. Cloud KMS is optional only for a later hosted service, not a V1 requirement. Encrypt backup archives before upload; design recoverable backup keys.
- Never collect/store banking passwords or card CVV; use approved financial aggregators and tokenized integrations; avoid handling full card numbers where possible.
- User-approved filesystem roots, explicit integration scopes, no stealth monitoring, activity indicators, audit logs, revocation controls, retention/deletion/export controls.
- Validate inputs, parameterized SQL, output encoding, CSP where applicable, SSRF protections for URL fetching, rate limits, malware/file scanning, supply-chain dependency scanning, secrets rotation, signed updates.
- MCP tool isolation, allowlists, timeouts, tool-result prompt-injection defenses, human approval for high-impact operations, per-tool audit logs.
- Separate personal and business domains and later tenants using strong authorization boundaries. Future public launch requires privacy policy, platform policies, data processing assessment and security testing.

## 8. Data entities
User, Device, Workspace, InterestProfile, Source, FeedItem, TrendSignal, Watchlist, Bookmark, Note, Pin, Task, Reminder, Notification, Contact, Organization, Lead, Opportunity, OutreachActivity, Company, AccountConnection, FinancialRecord, Event, Tournament, Expo, Model, Repository, MCPServer, ToolConnection, AgentMission, AgentRun, Idea, LearningPath, Project, ProjectRepository, ProjectFolder, ProjectSession, ProjectActivity, Milestone, Document, Attachment, AuditEvent, SyncChange.
All entities have stable IDs, created/updated timestamps, provenance, ownership/workspace, searchable metadata and appropriate access controls.

## 9. Key UX screens
Desktop: Home; universal command palette; unified inbox; Discover (GitHub/AI/Apps/YouTube/News); Robotics; Agri-tech; Poker; Opportunities/CRM; Projects; Notes; Tasks/Planner; Learning/Ideas; MCP Marketplace; Companies/Finances; Agent Missions; Settings/Integrations/Devices.
Mobile: Today; Ask AI; Capture; Tasks & Notes; Alerts; Projects; Saved; Approvals; Settings. Avoid recreating every desktop management screen.

## 10. Core workflows and acceptance tests
1. User says 'Find trending open-source underwater robots under $1,000'; app shows sourced results, cost caveats, and Save/Research/Start Project actions.
2. User selects a GitHub repo and says 'I'm starting this'; app creates project, links repo and asks permission to associate a local folder.
3. After approved local Git activity, dashboard shows actual latest commit, device, timestamp, current branch and next task; distinguishes detected facts from AI interpretation.
4. V1: project activity on each desktop is stored locally with device provenance. V2: after approved device pairing/sync, Windows shows last Mac session without pretending Mac-only files exist on Windows.
5. User dictates a note and says 'pin forever'; note appears in pinned dashboard until manually unpinned.
6. User says 'Remind me every Monday indefinitely to review AI models'; reminder persists locally and triggers while the device can run scheduled tasks. In V2, it synchronizes across paired devices without duplicate notifications where coordination is possible.
7. User asks 'Show Austin businesses with outdated websites I have not contacted'; app returns qualified leads with evidence and CRM follow-up status.
8. User asks 'Find upcoming poker tournaments over $1M guaranteed'; app shows dates, venue, buy-in and verified source, with watch/alert options.
9. User searches 'Unity MCP'; catalog lists verified and community options, risks and setup, without installing until approved.
10. Offline desktop can search cached items and add notes/tasks without any backend. In V2, after reconnecting to a paired device or relay, changes synchronize without silent data loss.
11. Sensitive financial/document records are unreadable without authorization and are not leaked into generic model prompts or feeds.
12. V1 mobile captures local voice notes, tasks and saved content, and retrieves live feeds over the internet. V2 mobile receives synchronized digests/project status and can request remote agent approvals when a trusted device is reachable.
13. V1 export creates CSV/JSON/Markdown plus a manifest-backed portable archive; exported records can be inspected in standard tools, and a clean install can import them without missing IDs, links, or attachments.
14. Sensitive full backups are encrypted and verifiable; a wrong password fails safely, and secrets/tokens are never exported. User can choose an OS-mounted cloud folder as destination without an app-owned backend.
15. App-controlled reset/uninstall offers a verified backup step and clear keep/delete choices; ordinary macOS Trash removal and mobile OS uninstall are explicitly documented as not interceptable. The UI never claims a backup succeeded without verification.
16. V2 paired devices on the same LAN exchange incremental changes securely without cloud infrastructure; simultaneous edits produce predictable merge/conflict outcomes and do not silently lose data.

## 11. Delivery roadmap (all domains retained)
- Phase 0 — local foundation: monorepo, desktop/mobile design system, per-device SQLite schemas and migrations, local secret storage, local AI adapters, direct API clients, source adapters, device ID/change-journal primitives, permission/security model, installer pipelines and tests. No Google Cloud provisioning.
- V1 / Phase 1 — usable standalone desktop: home, command bar, notes/tasks/pins/reminders, direct GitHub/LLM/news ingestion, local search, local AI, project workspace, read-only local Git/folder observation, daily briefing, optional API integrations, portable export/import, encrypted backup archive, and uninstall/reset data-protection UX. Desktop must function without a proprietary backend. Lightweight mobile can start with independent local capture and live source access.
- V1 / Phase 2 — domain breadth: YouTube/apps, robotics/agri/poker, learning/ideas, events/alerts, MCP catalog and safe tool execution; local CRM, LLC, email/calendar/finance modules and documents can expand iteratively without cloud dependency.
- V2 / Phase 3 — optional multi-device continuity: same-LAN P2P discovery/pairing first, incremental sync, conflict handling, mobile sync, scheduled encrypted backups to user-owned cloud/NAS, optional encrypted relay only if cross-network sync is later chosen; retain full local functionality when unavailable.
- V3+ / Phase 4 — optional always-on and commercialization: opt-in hosted scheduler/relay/push, advanced agent workflows, opportunity scoring, knowledge graph, content studio, extensibility, multi-user tenancy and app-store distribution. Google Cloud is a possible first deployment provider, cloud-agnostic by design.
Priorities sequence delivery only; no functional domain is removed from target scope.

## 12. Nonfunctional targets (initial engineering goals, validate with measurement)
- Local command palette and note capture respond within 300 ms on supported desktop hardware.
- Cached dashboard initial render within 2 seconds under normal conditions; external feeds load asynchronously.
- No backend availability dependency for V1. Optional V2 relay/backup failures must not prevent local work; define hosted-service SLOs only if such services are deployed.
- Background collectors respect API quotas, robots/terms, exponential backoff, retries, and per-source freshness indicators.
- Observability: on-device structured logs, error reports and local job health; V2 sync diagnostics. No mandatory telemetry or centralized collection; diagnostics export requires consent.
- Automated tests: unit, API contract, integration, sync/conflict, permission/security, desktop packaging, mobile smoke tests.
- Accessibility: keyboard-first desktop, screen-reader labels, contrast, adjustable font sizes and reduced-motion support.

## 13. Build instructions for coding LLM
Treat this PRD as the product contract. Start with Phase 0 and a strictly local V1 vertical slice: create note via desktop command bar → persist to local SQLite → pin → create recurring task → restart offline → verify both remain available. Then implement portable CSV/JSON/Markdown export, encrypted full archive, restore validation and app-controlled reset warning. Then connect a direct GitHub feed and a read-only local Git observation prototype, and demonstrate local AI search. Produce architecture decision records, monorepo structure, local database migrations, integration contracts, future sync event schema, threat model, runnable dev environment, automated tests, desktop installers/build pipelines and phased backlog. DO NOT provision Google Cloud, Cloud SQL, remote application API or hosted workers in V1. V2 pairing/sync/backup should be a separate opt-in milestone, not silently implemented in V1. Use mocks/adapters when API keys are absent. Never fabricate live trend data. Require explicit confirmation for privileged filesystem access, MCP tool execution, external communication, purchases, destructive actions and sensitive data access.

## 14. Open decisions (do not block scaffolding)
Brand/name; exact model providers; supported financial aggregators; SMS provider; optional backup destination and recovery-key strategy; LAN sync technology and peer pairing UX; whether an internet relay is ever needed; which connectors to prioritize; public-app licensing; final mobile feature cut; sync/backup retention; multi-user monetization. Use configurable interfaces and safe defaults until decided.

## 15. Architecture invariants and sync decisions
1. **Local is authoritative:** all user-generated notes, tasks, project metadata and personal history are stored locally first. Internet disconnection never blocks local creation or search.
2. **Cloud is optional:** no mandatory account, server, cloud database, or paid cloud subscription in V1.
3. **External internet is allowed:** live public information and third-party account integrations can use HTTPS directly; the application must disclose when data is transmitted to third parties.
4. **Sync is not backup:** V2 implements separately testable secure peer synchronization and encrypted backup/restore.
5. **Device-scoped data:** filesystem paths, installed models, local process/session metadata, and machine-specific secrets never automatically copy to other devices.
6. **No surprise uploads:** project documents, financial data, source code and personal attachments remain on the device unless a user explicitly enables a sync/backup scope.
7. **Cross-network reality:** same-Wi-Fi peer sync is feasible without owned cloud infrastructure; reliable internet sync generally requires reachable peers, NAT traversal and/or a minimal relay.
8. **Mobile limits:** iOS/Android background execution and LAN permissions constrain discovery and sync timing; do not promise continuous background peer sync.
9. **Security:** mutual peer authentication, encrypted channels, revocable trust, conflict handling and recoverable backup keys are mandatory before enabling V2 sync/backup.
10. **Modularity:** implement `LocalRepository`, `ExternalSourceConnector`, `SyncTransport`, `BackupProvider` and `AIProvider` interfaces so cloud capabilities can be added without changing core domain logic.

## 16. Cost and ownership invariants (v1.2)
1. No mandatory recurring cloud hosting costs for core app functionality.
2. All core user data remains on user-owned devices by default; the internet is for explicitly enabled discovery and third-party integrations.
3. Portable export/import is a **V1 requirement**, not deferred until peer sync.
4. Same-LAN encrypted peer-to-peer sync is the **preferred V2** multi-device strategy.
5. Scheduled backup to user-owned cloud is optional V2, not app-operated cloud storage.
6. Data deletion and uninstall messaging must respect real OS limitations and never imply that a prompt is guaranteed during OS-controlled removal.
7. Users can leave the product with their data in documented, usable formats, without a subscription or vendor permission.
