# Implementation plan and progress

## Current scope

The initial request was to study the attachments and start the project. The subsequent instruction was to continue implementing the remaining PRD and skip tests. That instruction supersedes the local testing convention for this pass. Build/compile checks were used to produce buildable artifacts; no runtime test suites or UI automation were run in v0.2.

PRD v1.2 and the conversation decisions remain authoritative. Original source files are unchanged under `docs/source/`; the historical cloud architecture is superseded.

## v0.1 foundation

Implemented notes/tasks/saved items, SQLite migration, keyword search, pins, recurrence, portable item archive, encrypted archive and import preview. Original frontend/core/native build results are retained in `ACCEPTANCE.md` as historical results only.

## v0.2 implementation

- SQLite migrations 2/3: typed workspace records with FTS and links, authorized device-local folders, discovery cache, encrypted vault records, notification delivery markers, embedding cache.
- Seventeen domain record configurations: project lifecycle, robotics/BOM, farm research, poker events/results, CRM pipeline/proposals, companies, learning, ideas, creator scripts, contacts, travel/events, assets/warranties, watchlists, MCP catalog, missions, observed sessions, notification history.
- Home daily overview, focus, unified inbox, cross-module command search, checklist editing, waiting/someday/yearly tasks, recoverable trash, activity history.
- System-wide capture shortcut, opt-in native due-task notifications and quiet hours; truthful background-execution limits.
- User-triggered public-source adapters for GitHub/HF/HN/MCP/Apple, plus YouTube with an optional OS-secure-store API key.
- Fixed-loopback local Ollama answers with selected records, explicit consent, request cancellation and mission records; semantic index/search with vault exclusions.
- Authorized single-root Git observation with bounded read-only process calls and revocation. No filesystem scanning, Git writes or model/editor inference.
- AES-GCM vault backed by OS secure-store key. Encrypted metadata/documents, bounded file picker import, explicit plaintext document export, session expiry.
- Portable schema 2 exports: ordinary records and history, selective scopes, optional encrypted vault documents, atomic writes, read-back verification and persisted export metadata. Import preview supports keep-local or separate-copy conflict handling with cross-link remapping.
- Data inventory, backup education, typed reset confirmations and honest OS-uninstall limitations.
- Independent SwiftUI and Kotlin/Compose companion projects: SQLite notes/tasks/saved items, search/pins/recurrence, on-device dictation and manual missing-only JSON transfer. No claim of automatic sync.

## Remaining work

The detailed gap list is maintained in [PRD-COVERAGE.md](PRD-COVERAGE.md). Remaining work is substantive: live domain-specific research and account integrations, sandboxed MCP execution/agents, full mobile breadth, secure paired LAN sync, scheduled cloud/NAS backups, richer scheduling/notifications, and signed production distribution. Do not mark the entire PRD complete because every domain now has a workspace screen.

No cloud backend was provisioned. No accounts, credentials, model installs, filesystem authorizations, messages, publications, or financial actions were performed on the user's behalf. Connector/key/permission flows are app features for explicit future use.

## Delivered artifacts for this pass

- macOS debug `.app` and plain DMG generated successfully.
- Native iOS Simulator app compiled without code signing.
- Android companion source/configuration supplied; compilation unavailable without Gradle.
- Tests and runtime verification intentionally skipped at the user's request.

These are development artifacts. PRD completion remains open as detailed in the coverage matrix; secure peer sync, account integrations, advanced domain automation and production distribution are not represented as finished.

## v0.3 continuation — 2026-11-14

Implemented dashboard controls, interest/read-state feed ranking with observed GitHub sample deltas, persistent AI conversations and citation-ID membership checks, permissioned scheduled source/local-brief recipes, atomic linked task/document plans, editable domain drafts and comparisons, relationship browsing, custom interval recurrence and snooze/escalation, opt-in periodic Git observation and folder resume, encrypted manual finance/bankroll/budget workflows, portable schema 3 preferences/feed caches, configurable backup education, encrypted scheduled local/mounted-destination backup with optional verified-file rotation, and live cached discovery on both mobile companions.

Detailed behavior and boundaries: [v0.3 delivery](V0.3-DELIVERY.md). The [HTML report](STATUS-REPORT.html) is updated from the 95-task scope dataset. No account was connected, source refreshed, real backup scheduled, private repository authorized, or vault opened during implementation. App features require the user's in-app authorization. No tests or runtime automation were run.

Gradle, Java and the Android SDK were discovered in existing machine caches during this continuation; the earlier missing-Gradle assessment is historical. Current build results are recorded in ACCEPTANCE.md. PRD completion remains open; the outstanding items above are not removed from scope.

## v0.4 continuation — V2 implementation, work in progress

Added storage migration 4 and causal event replication; Noise-authenticated one-time pairing and pinned encrypted channels; explicit desktop LAN listener and mDNS discovery; bounded incremental exchange, cursors, conflict UI, revocation and sync-key replacement; opt-in desktop polling and exchanged delivery receipts for acknowledged-task reminder coordination. Both mobile apps now use the shared Rust store/protocol, preserve legacy databases during migration, discover desktops, pair, sync, show project summaries and submit review-only plan approvals. C ABI/JNI libraries and native apps compile.

Added explicit category-by-category private desktop transfer with recipient approval and encrypted conflict copies. Added HTTPS WebDAV encrypted backup upload, read-back verification, OS-stored credentials, and conditional retention. No actual peer connection, private transfer, WebDAV upload, account connection or listener was started in development. Tests remain skipped. The source coverage report is being updated; built-in relay/NAT traversal alternatives, mobile vault, production signing and broader V1 integration gaps remain distinct from this implemented V2 core.

Reproducible mobile native staging: `scripts/build-mobile-core.sh`. This build supports ARM64 iOS Simulator/device and ARM64 Android; device signing and x86 simulator/Android ABIs are separate work.

### V2 scope closure in source

Completed optional foreground receiving on iOS/Android with native advertising, comparison-code approvals, lifecycle stop, and in-memory trust refresh after secure-store writes. This enables mobile-to-mobile connections while both apps are foregrounded. Added opt-in existing private mesh/VPN routing without provisioning infrastructure, content-free diagnostics export, and bounded notification receipt exchange. An elected peer that cannot actually deliver notifications no longer suppresses local alerts.

The eight tracked V2 tasks are source-complete within the chosen scope (existing private mesh as the optional relay alternative). The whole PRD remains incomplete: V1 domain/account integrations, richer local workflows, mobile vault/alerts, Windows delivery, signed releases and runtime acceptance retain their own open tasks. No source percentage is a claim that these gaps disappeared.

Final v0.4 packaging completed: macOS `.app` and DMG, iOS ARM64 Simulator `.app` with the Rust library linked statically, and Android ARM64 debug APK. Paths/checksums: [BUILD-ARTIFACTS.json](BUILD-ARTIFACTS.json). No tests or runtime acceptance were performed.

## v0.5 continuation — full PRD implementation in progress

Added migration 5 ordinary attachments; text extraction/search, original raster/text preview, export, version lineage and soft deletion; safe Markdown editing; weekday/end-date/prerequisite task recurrence; dependency-aware selective exports and portable schema 4 originals. Conditional tasks activate from desktop reminder polling and mobile snapshots. New attachments are included in archives/backups but are not yet part of ordinary peer replication.

Added opt-in arXiv/RSS/Atom collectors, OpenAI selected-context requests with exact payload consent, local Ollama lifecycle controls, visual knowledge graph, bounded approved project-manifest inspection, single-page CRM HTML audits, and read-only Google desktop OAuth with PKCE, OS-secure refresh tokens, inbox-header/calendar preview and explicit encrypted-vault copying. No account was connected or external action executed during implementation.

Rust workspace and TypeScript/Vite compilation pass as changes land. No tests or runtime acceptance were run, as requested. Previously packaged v0.4 artifacts do not contain these additions; rebuilding release artifacts is still outstanding. The HTML report remains an honest weighted source estimate across all 95 tasks, not a declaration of full PRD completion.

Remote MCP Streamable HTTP source is now implemented with exact endpoint/tool allowlists, device-secure bearer credentials, JSON/SSE handling, individual payload/hash/typed approvals, bounded requests, cancellation and content-free audit records. It does not start local processes or claim to sandbox remote servers; local stdio isolation, richer auth, installation and updates remain open.

Mobile vault source now shares desktop AES-GCM codecs and portable encrypted archives. iOS and Android have private-record editing, session/background locking, conflict-copy restore preview and password-encrypted vault-only backups. Keys remain in Keychain/Keystore-protected storage; missing keys never silently replace an existing vault. These additions compile on both native platforms. Android also marks the vault dialog as screenshot-protected. They remain untested at runtime; private peer transfer on mobile and notification breadth are separate remaining work.


## v0.5 delivery continuation — November 14, 2026

Implemented native mobile private vault/archive exchange and local reminders; desktop on-device dictation; encrypted invoices and reviewed HTML exports; bounded research/local-analysis plans; read-only authorized Plaid Item access; attributed OpenStreetMap discovery; separate-consent filesystem watchers; explicit publisher trust and signed update controls. Hardened private record writes with ciphertext compare-and-swap and checked plan/context review consistency.

Current source compiles for macOS desktop, shared mobile Rust, iOS Simulator and Android ARM64. No tests, application launches, account authorizations, microphone sessions, live tool calls or real updates performed. Report source estimate: full PRD 70%, V1 69%, narrowly scoped V2 100%, optional V3+ 8%. These are manually weighted source estimates, not validated completion. Remaining scope stays recorded in the task report.

## All-phase scope clarification and V3 continuation

The user explicitly requires every V1, V2 and V3 requirement; no phase is excluded from the completion target. Added optional hosted tenant authorization, role/ownership checks, operator-maintained license/seat policies, bounded always-on collectors, encrypted archive mailboxes, owner export/reset, public moderated extension catalog with publisher withdrawal, stateless GCS CAS adapter, OCI source and unapplied Terraform. Added desktop hosted/catalog controls, declarative REST/container extension builder and explicit execution approvals; no container was run. Added read-only GitHub App OAuth/PKCE and private snapshot controls. Added separately consented email/SMS/voice/ntfy personal reminders with daily caps, quiet hours, snooze and ambiguous-delivery handling; no messages/calls were sent.

Rust workspace and TypeScript builds pass for this source; optional host Python syntax compiles. Terraform is not installed here, so provider validation/planning was not performed. These source changes are still being integrated; macOS packaged artifacts predate this continuation until rebuilt. No tests run. Public identity onboarding, billing automation, native hosted push, domain-specific data feeds and other report gaps remain open; do not describe the entire PRD as complete.

## Further V1 completeness pass

Added deterministic Navigate/Answer/Act command routing with reviewed mutations, palette dictation, domain filters, prefilled source searches and selected-context questions; interface scaling/high-contrast/reduced-motion/skip-link controls; source-attributed iCalendar imports and watched schedule-change review alerts; official USDA/India agricultural observation adapters; optional local ClamAV scanning; explicit cache/history cleanup and aggregate diagnostic export. No calendar/commodity provider was queried with account credentials, no antivirus scan was run, and no runtime tests were performed.

Dependency advisory checks identified SQLite/ZIP issues; upgraded to patched packages and adapted archive decoding. npm advisory count is zero; Rust vulnerability-entry count is zero with five transitive maintenance/unsoundness warnings recorded separately. Core/desktop compile and UI build are the validation scope. Rebuild all packages after these dependency changes.

### Continued full-scope implementation

Added source-backed model metadata comparisons, rounded YouTube creator snapshot deltas, saved travel constraint comparisons, CRM pricing assumptions/HTML concepts, PDF text/PNG-JPEG OCR adapters, accessibility controls and page-level UI code splitting. Corrected the travel event category mapping, extension update cancellation/registry serialization, GitHub refresh persistence after revocation, reminder consent invalidation and post-reset event health writes. Added NSIS backup reminder and owner-controlled release signing/manifest preparation. These are implementation/build changes, not live provider actions or runtime acceptance.

All V1/V2/V3 requirements remain in the task report. Remaining material gaps include general recurring multi-step tool agents, attachment peer transfer, local stdio MCP/dependency installation, richer licensed market/tournament/subsidy providers, native push and cross-device delivery election, public self-service identity/billing, production signing/store release and independent security/performance/runtime acceptance. Do not label these as done merely because generic workspace fields or adapter scaffolding exist.

### Follow-through: recurring plans and local MCP

Implemented recurring bounded public/local agent plans and sandboxed stdio MCP containers after the preceding gap list was written. General interactive host-tool workflows and dependency installation remain separate. Current Rust/TypeScript builds pass. Optional Terraform syntax and provider schema validation pass after correcting malformed HCL blocks; no infrastructure actions occurred. macOS, iOS Simulator and Android development packages are being refreshed; final paths/hashes belong in BUILD-ARTIFACTS.json.

### Mobile backup picker lifecycle correction

Source review identified that the system file picker can background/lock the vault before its callback returns. Both companions now retain only encrypted export/import bytes across that lock, clear keys/passwords/plaintext as before, and defer restore preview until the user unlocks and re-enters the recovery password. Selection alone never decrypts or imports. Native packages were rebuilt; picker lifecycle behavior remains untested at the user's request. MCP server error messages are also omitted from durable audit, and unconfirmed MCP container cleanup is surfaced even after a successful response.


### Latest remaining-scope pass

Implemented transient consented account views and mission review in Inbox; licensed Android/iOS market snapshot import and comparison; company-linked overviews and obligation tasks; evidence-labelled lexical graph suggestions; provider cooldown/Retry-After enforcement; paginated sandbox MCP discovery; separately approved immutable Docker dependency downloads and field-level extension update review. Analysis-note saves refresh the shared workspace. Busy extension selection is locked so cancellation targets the reviewed job.

Rust/TypeScript compile checks and all four development artifacts rebuilt successfully. No test suites, app launches, account calls, dataset imports, Docker pulls/tool execution, messages or deployments performed. Updated HTML/JSON coverage: 81% overall, V1 80%, narrow V2 100%, V3+ 50%. Remaining functionality and production/runtime acceptance remain explicitly open; no 100% product claim. BUILD-ARTIFACTS.json records current hashes.


### Authorized attachment sync and native push pass

Implemented attachment scope on desktop/mobile pairing, owner-category gating, full original/checksum/version/trash/index replication through causal events, one-file batches, transactional commits, file-level retry from acknowledged progress and explicit conflict choices. Companion attachment screens search metadata/indexed text and export originals through native destination pickers. Limits: 10 MiB originals, 1,000 versions, 32 MiB authenticated messages, no byte-offset resume; causal history duplicates file bytes. Vault files remain separate.

Implemented opt-in APNs registration/entitlement source and FCM native SDK/service, token refresh/removal, generic notifications, Android dedupe/quiet hours, encrypted device credentials, optional host encrypted token registry, fixed provider adapters, send deduplication/rate bounds, tenant/subject access, and desktop native-push reminder channel. Added owner setup documentation and resolved Python dependency lock. Hosted reset/export handles push metadata without exporting tokens. Commercial onboarding and workspace tenant authorization changes are outside this user-authorized pass.

Rust/TypeScript, shared mobile libraries, iOS Simulator and Android debug builds pass. Python syntax and dependency compatibility pass. No tests, application launches, real pairing/sync, provider authorization, push send or deployment performed. Native push cannot be called active until the owner configures provider credentials, a reachable HTTPS host and signed Apple capability. See ATTACHMENT-SYNC.md and NATIVE-PUSH.md for exact boundaries.


### Authorized Mac/Windows testing

The user explicitly requested tests, replacing the earlier skip-tests instruction. Added eight causal/attachment integration cases and two encrypted localhost transport cases shared by desktop/mobile; existing seven core/archive tests and ten new mocked push-server cases pass. Verified frontend/native compilation and read-only Mac DMG mount/copy integrity. Repaired incomplete Mac development resource signing in package-mac-plain.sh; strict verification passes but production Gatekeeper acceptance still fails. Windows workflows now invoke all workspace tests before packaging; no remote/Windows runner is configured, so Windows is untested. The initial native UI attempt was blocked by the locked screen; see the follow-up below. Full results/logs: docs/TEST-REPORT.html and docs/testing/. No live account/provider actions or production-data mutations occurred.

## Native Mac QA follow-up — 2026-11-14

Unlocked-Mac testing exposed a startup crash caused by missing updater plugin configuration. Added explicit empty updater defaults and a deserialization regression test; the isolated QA app launches. Verified note capture/edit/pinning/search, attachment import and text preview, monthly Jan 31 → Feb 28 recurrence, full quit/relaunch persistence, readable and encrypted archive exports, duplicate-safe imports and wrong-password rejection. Independent ZIP checksum and original attachment checks pass. Corrected stale version and attachment-sync UI text. Rebuilt the normal Mac app/DMG. Full Rust suite now has 20 executions (18 distinct cases); existing 10 mocked push cases pass. No real profile or accounts were used. Windows and production signing/notarization remain outstanding.

## Task reminder controls — 2026-11-14

Added device-local default/daily/hourly/once/muted desktop reminder policies, occurrence acknowledgement/resume, snooze status and 15-minute/1-hour/4-hour/1-day snoozes. Global consent and quiet hours still gate notifications; other channel digests are separate. Acknowledgement stores the saved due date and leaves completion unchanged. Stale revisions are rejected. Three isolated integration tests cover persistence, next-date eligibility, cadence, snooze and invalid targets. Frontend/native compilation pass; native UI validation of the new controls and after-quit scheduling remain open. HTML report now distinguishes external blockers from local engineering work and removes obsolete no-tests/integration-absent claims.

GitHub publication requested: destination https://github.com/ashishyeole/commandcenter, branch main. Repository was empty on inspection. Push-triggered Mac/Windows checks are configured; results must be established separately. Local reminder build and all 23 Rust executions pass.

## Discovery-first scope revision — 2026-11-14

Applied the user clarification in docs/LOCAL-SCOPE.md. Added bounded metadata-only Ollama catalogue ingestion with defensive parser tests; expanded HF count observations; added research-topic shortcuts; changed robotics workspace/draft language to public research. Removed local runtime-management and inference IPC exposure, replaced the Assistant with optional reviewed cloud AI, and changed automation/agent analysis to deterministic selected-record briefings (no local model required). Added briefing tests for selection/exclusion and bounds. Reassessed every task, preserving original scores and clearly excluding local inference, commercialization, release and verification from the new denominator. Local-scope estimate is 89%; this is not a 7-point implementation gain over the earlier 82% broad PRD. Additional code and integration work remains explicitly listed.

## Personal functionality continuation — 2026-11-14

Added named multi-source research collections (30 topics, 12 queries/topic), independent bounded query caches (100 snapshots), offline merged results, and cache cleanup integration. Approved refreshes refresh collection views without losing unsaved collection edits. Added saved dashboard widget ordering and publication/title sorting. Workspace records can belong to a project/company; owners are explicit links, missing/non-project parents and cycles are rejected, and the workspace list filters by parent. This is personal organization, not multi-user authorization. Graph recall searches content, tags and fields with up to three saved-link hops.

Added explicit opt-in numeric watch monitoring for GitHub stars / HF download counts from approved cached queries. Matching thresholds transition a watchlist to triggered and create one Inbox review record; no send or purchase is performed. Arbitrary prices are not parsed as verified numbers. MCP health derives from past reviewed requests without background network probes. Simple all-day daily/weekly calendar recurrences with bounded COUNT/date UNTIL are expanded; timed/complex/exception rules remain unexpanded and labelled. New repository integration tests cover persistence/bounds, thresholds/idempotence, ownership and recurrence. Broader adapters, fine-grained policy, after-quit scheduling and large streaming archives remain code gaps.

Final checkpoint validation: 36 Rust executions (34 distinct tests), frontend build and Mac compilation pass. Requested additional email/calendar provider and official tournament source preferences; no credentials were read or connections made. HTML code estimate is 91%. Remaining code gaps are preserved, including large streaming archives, after-quit reminders, complex event recurrence, provider breadth, fine-grained policy/retention and mobile private-file UX.

Added opt-in macOS LaunchAgent / Windows Task Scheduler reminder registration, hidden --reminder-check startup mode and a shared SQLite lease to serialize concurrent processes. The scheduled mode skips ordinary services and exits after checking tasks; it requires saved enablement. Disabling revokes permission even if OS removal fails. No scheduled job was installed by the agent. OS registration/display remains acceptance work.

Reminder QA: disabled and enabled headless checks exit 0 in the synthetic QA profile with notifications disabled. No OS schedule was installed or production profile touched. Sandbox-only launches aborted inside macOS application registration; unrestricted native QA invocation passed. Final suite targets 39 executions including invalid watchlist configuration and shared reminder lease cases; OS scheduling/delivery acceptance is distinct from code completion.

## Google-only, poker searches and floating online assistant — 2026-11-15

Applied user choices: keep Google-only email/calendar. Added worldwide/provider-specific tournament search planning, location/game phrases, satellites, dynamic year rollover and publisher shortcuts. Public publisher references are recorded in POKER-DISCOVERY.md. Calendar results merge all configured category sources by default and filter ended/cancelled events while retaining ongoing multi-day events; saved imports preserve end dates and exclusive-end semantics. Fixed stale recurrence labels. These are public search queries and imported-calendar filters, not exhaustive live aggregation.

Added an online-only floating assistant with circular draggable AI launcher, persisted/clamped position, keyboard movement and a contextual panel using existing reviewed cloud requests/secure-store setup. Preserved prior local assistant, runtime and inference source in dormant directories outside active module/build paths; no local model is installed or run. No provider account connected and no key entered during development.

Native QA follow-up: floating panel opened, pointer drag moved launcher/panel, keyboard moved/reopened, position survived quit/relaunch, and tournament search plan produced six source queries. Removed duplicate workspace integration panels exposed by this inspection. Event visibility updates every minute while open. No provider credential, cloud call, browser search or local model was used. See testing/floating-poker-native-qa.json.

## Streaming encrypted archives — 2026-11-15

Implemented PCCSTR01 per-record encrypted streaming export, full verification preview and transactional missing-only import, with authenticated framing/footer, reviewed-file checksum binding, UUID/type/size validation, rollback on corruption/truncation/cancellation, atomic publication and reset-epoch guards. Includes full ordinary records/history/attachments/configuration and optional vault entries re-encrypted with the receiving key; permissions and credentials are excluded. Added Data & privacy controls. Fixed standard portable validation to accept all nine current feed sources including Ollama. A real >64 MiB export/import test and additional integrity/relationship/vault tests pass. Advanced large-archive conflict-copy merge remains open. Native UI verification follows before publication.

Added optional Gmail body preview under separately approved gmail.readonly authorization. Metadata remains default, body requests require explicit message-level action/read permission, and returned content remains transient unless saved to vault. Plain text is preferred; HTML is converted to inert text with script/style removal and structural/size limits. Local granted-scope use is restricted to the scopes the owner selected. Added synthetic MIME, truncation and scope tests. No live Google account was connected.

Checkpoint verification: 53 workspace Rust executions plus one additional native cancellation test pass; native and frontend builds pass. The initial streaming-dialog attempt was blocked by the locked Mac; the unlocked follow-up below supersedes that blocker. Existing native QA evidence remains valid for its listed scenarios. Streaming source/code is implemented; advanced large merges and scheduled streaming destinations remain open. Mobile artifacts were not rebuilt for these desktop changes.


## Unlocked streaming restore and linked copies — 2026-11-15

The unlocked isolated Mac QA app exported three synthetic records, rejected a wrong password, previewed three existing IDs and four history events, then imported zero missing records while preserving existing records. The earlier Mac-lock blocker is resolved. Added explicit all-incoming linked-copy restore for large archives: fresh current-record IDs/titles, remapped links/owners/task prerequisites/attachment versions/history, receiver-key vault copies and unchanged local records/configuration. Two authenticated passes are bound to the same encrypted-file checksum; failures roll back. Three new repository tests pass; final workspace/native QA results are recorded in testing/results.json. Per-record selection/field merging and scheduled streaming destinations remain open.


Added optional scheduled streaming backups to approved local/mounted folders. Legacy schedules keep standard format; selecting streaming clears approval. Native writer uses encrypted temp files, flush/fsync, full verification and no-clobber publication; retention hashes in chunks. Synthetic writer/backward-config tests pass. Direct WebDAV remains standard format; actual unattended keychain-backed scheduling is unverified.

Expanded bounded public-calendar recurrence to all-day, UTC, IANA and floating daily/weekly/monthly/yearly rules, selected weekday/month-day/month/week-start filters, EXDATE and RDATE. Preserves local wall-clock time across DST and exact DTEND duration; invalid/ambiguous local times are omitted, and unsupported/unbounded/overridden series remain explicitly unexpanded. Seven parser integration tests cover month-end skipping, leap years, ordinal weekdays, interval weeks, DST, duration, exclusions/additions, inclusive UNTIL and safe fallback. No source URL was fetched during these tests. See docs/CALENDAR-RECURRENCE.md for limits.


Final checkpoint: 66 workspace Rust test executions (64 distinct), TypeScript/Vite and native Mac build pass. Development app/DMG rebuilt; ad-hoc signature and DMG checksum verification pass. Mac streaming export/preview/password rejection/missing-only import/linked-copy restore passed in the isolated QA profile; that lock blocker is resolved. The scheduled writer and new calendar recurrence parser have automated synthetic coverage, not native/provider acceptance. Mobile binaries were not rebuilt. GitHub CLI has no authenticated session, so Windows CI status could not be checked; no Windows executable/runtime evidence exists. The weighted code estimate remains 92%: these additions reduce partial-task gaps without claiming unsupported rules or remaining engineering work complete.


## Local installer completion follow-up — 2026-11-15

Confirmed d46c26c checks passed on both macOS and Windows through the existing repository Git credential helper; credentials were never printed or persisted. This supersedes the earlier unauthenticated-GitHub-CLI limitation. Added Windows NSIS install/native-window/uninstall smoke checks to the opt-in packaging workflow.

Implemented explicit Google background read permission, per-scope validation, bounded intervals, first-page header/calendar collection, 30-minute memory-only snapshots, status/stop controls and Inbox display. Configuration changes, disconnect, reset and TTL invalidate snapshots; background reads never fetch message bodies or enter ordinary storage, search, export, sync or AI. Defaults remain off. Two synthetic privacy tests pass; actual OAuth/provider testing still needs owner setup.

Added Research radar with curated robotics/agriculture queries, MIT/NASA/USDA and official India scheme sources, cached publication windows, within-provider comparable growth percentiles, lexical cross-channel creator research suggestions and evidence-linked idea/content drafts. Thirteen synthetic analytics assertions and frontend build pass. These are explicit local-sample heuristics, not market-wide growth or verified demand. No model or hardware is downloaded or run.


Final follow-up evidence: 68 Rust executions (66 distinct), 10 mocked push tests and 23 date/position/research assertions pass; frontend and Mac compilation pass. The first push-test rerun used the system Python without crypto dependencies; rerunning in the existing isolated pcc-push-deps environment passed all 10 cases. No provider calls were made.

Packaging workflow 43786808410 passed on both platforms for ef1e9c3. Windows NSIS silently installed, created the native app window and stayed alive for 15 seconds, then silently uninstalled with exit code 0. Downloaded the installer and verified its SHA-256 against the CI report. This supersedes the earlier lack of Windows executable/runtime evidence; it remains a smoke check, not every-feature acceptance. The local Mac development DMG was rebuilt and passes checksum/signature verification.

Implemented mobile private-file attachment/replacement/removal in reviewed drafts, bounded inert text/image previews with no plaintext temp files, and Android read-after-unlock handling for the external picker. iOS Simulator and Android ARM64 debug builds pass using existing SDKs/cached dependencies; no device was launched. File-provider/lifecycle acceptance remains unverified. The latest Mac Google/radar native checks are blocked by the locked screen; an unlock request is pending. Earlier selected native Mac QA remains recorded only for its tested workflows.


## Appearance and native QA follow-up — 2026-11-16

The Mac unlock blocker is resolved for the selected Google/radar checks. Isolated synthetic QA verified default-off Google scope/body/background/recurring controls and stop/clear without account setup. Radar counts/score matched four fixtures; research and creator drafts saved locally, and query preparation kept fetch approval off. No real account or AI request was made.

Implemented four persistent desktop themes: Monochrome (silver/black/gray with Geist Mono), Lumen (warm paper/teal), Slate (cool gray/blue), Midnight (charcoal/violet with Geist Mono). Geist Sans/Mono variable fonts and OFL license are bundled. Removed visible AI text from the draggable circular launcher, retaining its accessible name and image. Grouped sidebar navigation; sectioned Settings, Discover and Data & privacy; compacted source and workspace-domain selectors. All controls/details remain available, with provenance under a disclosure and mounted tab panels preserving drafts. Source thumbnails retain full color and use only fixed public YouTube/GitHub CDN destinations with no referrer or forwarded query data.

Native QA rendered all four themes, checked restart persistence, full-color sample thumbnail, assistant open/drag/persistence, all section groups, keyboard tab movement and an unsaved password surviving a section switch before clearing it. Connections has no overlapping general Save control. Automated frontend build, 69 workspace Rust executions (67 distinct tests), 12 new source-image/theme assertions and four-theme portable archive roundtrip pass. Existing 23 UI/analytics assertions and 10 mocked provider tests remain recorded. No full performance/accessibility/provider/device acceptance claim. Functional estimate remains 94%; the appearance request does not close unrelated PRD gaps. Installer rebuild provenance is recorded in BUILD-ARTIFACTS.json.


Final appearance delivery: source 96c7403 passed macOS/Windows desktop checks (43915401780) and both release packaging jobs (43915437331). The Windows installer silently installed, produced a native window for 15 seconds and silently uninstalled; its downloaded hash matches the CI report. Replaced the local Windows EXE. The local Mac development DMG was rebuilt, checksum-verified, mounted read-only and strict-deep-signature-verified, then copied with a matching hash. Both delivery files now contain the new desktop appearance. Mobile UI artifacts remain at their separately identified private-file source revision; desktop themes are not a mobile redesign. Full feature/device/provider acceptance remains open.
