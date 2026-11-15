# Personal Command Center

A local-first desktop workspace built from PRD v1.2: Tauri 2, React/TypeScript, Rust and SQLite. No mandatory backend, sign-in or cloud subscription. Version 0.5 adds licensed Android/iOS market comparisons, company overviews, reviewed graph suggestions, explicit container dependency setup, scoped attachment peer sync, native APNs/FCM push support, research feeds, public model catalogues and optional cloud AI, read-only account connections, reviewed MCP tools, recurring research plans, isolated stdio MCP, PDF/OCR indexing, travel comparisons, local website concepts, mobile vaults/reminders, desktop dictation, invoices, business discovery, project watchers and signed-updater controls. Optional hosted tenant services and a moderated catalog are supplied as unapplied deployment source. Existing authenticated desktop/mobile sync and encrypted backups remain available. See [v0.5 implementation boundaries](docs/V0.5-DELIVERY.md).

**Current scope: a personal discovery and organization command center. No local LLM downloads, robotics execution or prototype execution.** See [scope clarification](docs/LOCAL-SCOPE.md). This remains an implementation in progress. See the explicit [PRD coverage matrix](docs/PRD-COVERAGE.md). Selected automated storage/archive/sync/transport/push-policy tests now pass. Selected native Mac UI workflows pass in an isolated QA profile; Windows runtime remains blocked; see the [test report](docs/TEST-REPORT.html).

Open the offline [HTML status report](docs/STATUS-REPORT.html) for 95 task-level completion estimates, remaining work, evidence and notes. It includes filters, CSV export and a print/PDF view. Estimates describe implementation scope, not verified release readiness. Update `docs/status-report-data.json` and run `python3 scripts/build_status_report.py` to regenerate it.

See [attachment sync](docs/ATTACHMENT-SYNC.md), [native push setup](docs/NATIVE-PUSH.md), [current delivery boundaries](docs/V0.5-DELIVERY.md), [release preparation](docs/release/RELEASE-PREPARATION.md) and [optional hosted service](services/optional-host/README.md).

## Desktop

```sh
npm ci
npm run desktop
```

Prerequisites: Node 22+, stable Rust, Xcode command-line tools on macOS or Microsoft C++ Build Tools / WebView2 on Windows. `npm run dev` is a read-only browser design preview; it does not substitute browser storage for SQLite.

Available workflows:

- **Today / Inbox:** local daily overview, focus mode, pins, due tasks, workspace dates, notification history, mission reviews and consented transient Google inbox/calendar views.
- **Notes / Tasks / Saved:** editing, pins, priorities, waiting/someday, checklists, persistent tasks, daily/weekly/monthly/yearly recurrence, trash and recovery.
- **Command palette:** Cmd/Ctrl+K in-app; Cmd/Ctrl+Shift+Space system-wide while the app runs. Search titles/text, navigate, or try `note: an idea` and `remind me every Monday to review AI models`. Commands open an editor for review.
- **Workspaces:** projects, robotics, agriculture, poker, CRM, companies, learning, ideas, content, contacts, events/travel, assets, watchlists, MCP catalog entries, agent missions and project sessions. Structured fields, tags, cross-links, status pipelines, linked task/project creation and CRM/poker totals. Domain-specific automation beyond these workflows is not implied.
- **Discover:** opt-in GitHub repository search, Hugging Face models, Hacker News, MCP catalog, Apple app charts, and YouTube search using your API key. Cached results and source timestamps remain available offline. None of these snapshots claims measured growth velocity.
- **Assistant:** optional, explicitly reviewed cloud AI requests. Model discovery reads public Hugging Face/Ollama metadata without downloading weights. Automations and research plans compile selected saved records without a model.
- **Project observation:** select and authorize an individual Git repository root, then explicitly observe branch, tracked-file changes and recent commits. Revoke access anytime. This does not infer your editor/model or modify a repository.
- **Vault:** explicitly unlock the OS-keychain-backed AES-GCM vault for financial metadata, private company information and documents up to 10 MiB. Sessions expire after ten minutes. No banking passwords/CVV or account tokens belong in record fields.
- **Data & privacy:** selective or full ordinary exports, history, optional encrypted vault export, atomic writes, read-back verification, import preview, duplicate/conflict detection, keep-local or separate-copy conflict handling, device data inventory and explicit reset confirmation.
- **Settings:** module visibility, opt-in notifications and quiet hours, optional YouTube API key in the OS secure store. Notifications check once per minute while the app is alive, not while the device is asleep/off.

## Build / package

```sh
npm run build
cargo check -p personal-command-center
npm run tauri -w apps/desktop -- build --debug --bundles app
npm run package:mac       # macOS DMG
npm run package:windows   # Windows NSIS EXE, run on Windows
./scripts/package-mac-plain.sh  # DMG fallback without Finder automation, after debug app build
```

A macOS `.app` is produced under `target/debug/bundle/macos/`. The current unsigned debug DMG is `target/debug/bundle/dmg/Personal Command Center_0.5.0_arm64.dmg`. Release signing/notarization and app-store submission need your certificates and are not configured. The repository includes desktop CI and a manual packaging workflow; no remote workflow or publication was triggered.

Tests exist from the original foundation (`cargo test -p command-center-core`) but were **not run for v0.2, v0.3, v0.4 or v0.5**, per the latest user instruction. Old passing results do not validate the new code.

## Mobile companions

- [SwiftUI iOS project](apps/mobile-ios/README.md): independent SQLite capture, notes/tasks/saved items, pins, recurrence, on-device dictation, cached public discovery and manual JSON exchange.
- [Kotlin / Jetpack Compose Android project](apps/mobile-android/README.md): local capture, manual exchange, on-device speech and cached public discovery. The compiled debug APK is at `apps/mobile-android/app/build/outputs/apk/debug/app-debug.apk`.

The mobile projects are early companions, not App Store / Play-ready products. Desktop **Data & privacy → Mobile companion exchange** exports ordinary items as plaintext JSON. Import keeps existing destination IDs. This is not peer sync and cannot merge concurrent edits. Encrypted vault data never enters this mobile exchange.

## Local storage and privacy

Desktop databases are under Tauri's app-data directory:

- macOS: `~/Library/Application Support/com.personal.commandcenter/`
- Windows: `%APPDATA%\com.personal.commandcenter\`

Ordinary notes, workspaces, caches and history are plaintext SQLite. Vault record contents and filenames are encrypted; their random IDs and modification times remain visible. Device keys are in macOS Keychain or Windows Credential Manager. Unlocking uses the OS credential store; it is not a separate biometric/PIN challenge and does not defend against a compromised signed-in OS account.

A full private backup must explicitly include the vault and use a password. Export does not contain OS keys, API keys, local-folder permissions or embedding caches. If the device key is lost, restore from a password-encrypted backup; without either key or backup the vault cannot be recovered. See [portable formats](docs/PORTABILITY.md) and [security boundaries](docs/SECURITY.md).

No credentials were configured, no external account was accessed, no model was installed, and no local project folders were authorized during implementation.

## Source of truth and project structure

Original attachments are preserved under `docs/source/`. PRD v1.2 supersedes the older cloud-first conversation. The attached instructions were project context, not permission to access unrelated accounts or systems.

- `apps/desktop/src`: React screens and domain configuration.
- `apps/desktop/src-tauri/src`: narrow IPC, source adapters, local AI, authorized Git observation, secure vault and native dialogs.
- `crates/core`: migrations, SQLite repositories, history, archive formats, recurrence and encryption.
- `apps/mobile-ios`, `apps/mobile-android`: native independent companions.
- [Progress](docs/PLAN.md) · [Coverage](docs/PRD-COVERAGE.md) · [Architecture](docs/ARCHITECTURE.md) · [Build record](docs/ACCEPTANCE.md)

API implementation references: [Tauri plugins](https://v2.tauri.app/plugin/), [Ollama chat](https://docs.ollama.com/api/chat), [Ollama embeddings](https://docs.ollama.com/api/embed), [Hugging Face API](https://huggingface.co/docs/hub/api), [YouTube search](https://developers.google.com/youtube/v3/docs/search/list), [MCP registry](https://modelcontextprotocol.io/registry/about).

## V2 device continuity (v0.4 implementation)

Open **Device sync** on desktop to explicitly enable its LAN listener, choose ordinary-data categories and create a one-time invitation. On another desktop or mobile companion, discover the candidate (or enter its private IPv4 address/port), paste the invitation, compare codes and approve on the inviter. Sync now exchanges bounded causal changes; automatic desktop polling is optional. No service is enabled merely by installing the app.

Both companions now use the shared Rust SQLite and peer library. Build it before the native app:

```sh
scripts/build-mobile-core.sh ios-simulator
# Or ios-device before an iPhone build; sign with your own Xcode team.
ANDROID_NDK_HOME=/path/to/ndk scripts/build-mobile-core.sh android
```

Install the matching Rust target using your toolchain. If your Rust distribution supplies matching library sources instead of target binaries, `PCC_BUILD_STD=1` rebuilds std from them. `PCC_OFFLINE=1` uses cached dependencies. The current staging targets ARM64 only. Switching iOS SDK requires restaging the corresponding library. Legacy mobile databases are retained after one-time missing-only migration.

Private desktop category transfers require both vaults unlocked and recipient approval for every transfer; conflicts create incoming copies. HTTPS WebDAV scheduled backup is configured independently under Data & Privacy, with credentials in the OS secure store and archives encrypted before upload. Create the WebDAV folder on your server first. No provider account was connected in development.

Read [V2 protocol and limitations](docs/V2-SYNC-DESIGN.md) and the [HTML task report](docs/STATUS-REPORT.html). Tests remain intentionally skipped. Cross-device security/acceptance, built-in relay/NAT traversal alternatives, mobile vault/notifications and production signing remain open.

For phone-to-phone sync, open Device continuity on the receiving phone and enable **Receive on this mobile device**. Discover it or enter its LAN address and displayed port on the other phone, transfer the invitation privately, and compare/approve codes. Keep the receiving screen in the foreground; leaving it or backgrounding stops the listener. Trust persists securely for the next session. Restarting receiving changes the port, so discover/select the current endpoint.

For an existing private VPN, opt into private mesh mode on both devices and enter the virtual IPv4 endpoint manually. The app does not create network access or bypass VPN/firewall policies. No globally routable public endpoints or router port-forwarding mode are enabled.
