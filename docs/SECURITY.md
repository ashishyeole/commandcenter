# Security boundaries — implementation, not audit certification

## Data classes

Ordinary notes/tasks/workspaces, history, caches and vectors are plaintext SQLite. Sensitive financial/company metadata and document bytes belong in the separate vault table. Vault ciphertext uses AES-256-GCM with fresh random nonces and the record UUID as authenticated additional data. Only random IDs and modification timestamps remain visible. A random 256-bit device key is stored in macOS Keychain or Windows Credential Manager, never SQLite or export files.

Vault unlock is explicit and expires after ten minutes; leaving the Vault UI clears its state and locks the backend session. Unlock uses OS credential-store access, not an independent biometric challenge. OS/account compromise is outside this boundary. Native key buffers are zeroized, but JavaScript strings and deserialized record objects cannot promise complete forensic memory erasure.

Password-encrypted backups can include vault contents after explicit unlock. They never contain OS keys, YouTube API keys or local folder approvals. Native commands reject plaintext sensitive imports. Losing both the OS key and private backup makes recovery impossible.

## IPC / renderer

React text escaping, local-only CSP, no raw HTML, no unrestricted shell/filesystem/SQL plugin permissions. Native dialogs choose import/export paths and authorized project roots. External URL opening validates HTTP(S) and prohibits embedded credentials. User actions can open URLs in the system browser; data is not fetched merely by rendering record text.

Renderer compromise remains a meaningful risk: custom app commands are available to the trusted local window and can modify ordinary records or invoke an OS vault unlock. Release hardening must include broader threat review and explicit authorization design before financial transactions or arbitrary external tools are introduced.

## Network / AI

Public discovery uses fixed HTTPS providers, no redirects, timeouts and bounded response bodies. Refresh is explicit, with provider disclosure. Public search text is sent to the chosen provider, not private workspace records. YouTube keys remain in OS secure storage; API errors remove URL details so query keys are not displayed. Quota/backoff and provider terms still need release review.

Ollama uses fixed `127.0.0.1:11434`, no proxy/redirects, bounded requests/responses and local-model checks. Models marked remote/cloud are rejected. Answers receive only user-selected ordinary records; semantic indexing has a separate explicit ordinary-data consent. Vault content is excluded from both. Model text cannot trigger tools or claim execution. Sources are supplied context, not automatically verified citations. Cancelling drops the client request; server-side model memory/lifecycle is controlled by Ollama.

## Project observation

Only a native-picked Git repository root receives a device-local authorization. Observation runs fixed read-only Git commands with optional locks, fsmonitor and hooks disabled, bounded output and a timeout. No shell interpolation, disk-wide scan, writes, source upload or claimed inference about IDE/model usage. Revocation removes the binding. Repository metadata remains untrusted text.

## Portability / deletion

Validation, duplicate checks, bounded ZIP expansion, hashes and AEAD authentication precede writes. Import is transactional and does not silently overwrite conflicts. Exports are atomically persisted and verified before marking success. Archive signatures do not establish authorship; hashes establish integrity only.

Reset requires typed confirmation, with an additional phrase when the vault has records. It clears app records/history/caches, checkpoints WAL and vacuums with secure-delete enabled. This does not guarantee forensic erasure on SSDs, OS snapshots or backups. Original files, linked repositories, exports, OS credential keys and other devices are not deleted. The UI never claims to intercept macOS Trash or mobile uninstall.

## Mobile

Native companions store ordinary data in app-private SQLite and use explicit document pickers for transfer. They are not sensitive-data vaults. Android excludes automatic OS backup in its manifest; iOS applies device file protection to the database. Speech uses on-device-only APIs, requires microphone/speech consent, and has no intended cloud fallback. Runtime platform behavior has not been tested in this implementation pass.

## Remaining release gates

Independent security/runtime testing, threat review of import/conflicts/key recovery, dependency security workflow, signed installers/updates, account-provider OAuth, audited tool sandboxing, scope-specific permissions, retention controls, malware scanning, performance/resource limits at scale, and authenticated peer-sync design. None of these is implied complete by a successful build.

## v0.3 saved job permissions

Recipe configuration stays device-local and explicitly approves a public query or fixed selected record IDs sent to local Ollama. Scope changes require renewed approval. Jobs do not execute arbitrary tools or external messages. Reset increments an epoch checked before asynchronous source/AI writes, avoiding restoration of results after data reset.

Scheduled backups are opt-in. The recovery password is stored in OS secure storage, never exported. A separate displayed permission authorizes background vault-key access for encrypted backups even while the interactive vault is locked. Revoking that saved backup permission stops future vault inclusion; ordinary UI lock only ends interactive access. Initial authorization requires an unlocked vault. Rotation defaults off and checks recorded destination/path, regular-file type, size and ciphertext hash before removing an older owned backup. No claim of independently reviewed security is made.

Persistent AI chats resend up to six prior question/answer turns with current selected context; prior answers may contain information from earlier selections. UI discloses this. Citation-ID membership checks are not factual-grounding verification. Imported feed/preference payloads are validated and cannot grant notifications, filesystem access, scheduler or provider permissions.

## v0.4 peer and remote-backup boundaries

Ordinary sync uses pinned Noise identities, one-time invitation secrets and comparison-code approval, per-category intersection, transactional causal events and explicit conflicts. Mobile receiving is opt-in and tied to foreground lifecycle. Private mesh mode accepts existing virtual private routes, not globally routable endpoints; changing desktop network policy stops connections. Trust refresh/revocation is checked during exchange. Diagnostic export omits content, labels, IP addresses and cryptographic material.

Private category transfer is a separate authenticated operation with a distinct prologue, unlocked vaults, per-transfer recipient approval, bounded messages, and atomic recipient encryption. Divergence creates incoming copies; private contents never enter the ordinary causal journal. This is not automatic vault replication or remote erasure.

WebDAV backup uses explicit HTTPS destination approval, OS-stored app credentials, no redirects, ciphertext-only uploads, bounded read-back verification and conservative retention using recorded hashes plus strong ETags/If-Match. A remote server can deny service or retain uploaded ciphertext. Recovery passwords must be kept independently. Paired devices can retain previously shared data after revocation.

No current-version cryptographic, adversarial, interop or restore acceptance has been run. Compiling the code does not establish these properties at runtime. Independent review, journal-growth policy, signed releases and wider V1 security gaps remain open.

## November 14 continuation: dependency and retention controls

Public advisory scans found vulnerable bundled SQLite and ZIP versions. The core now uses rusqlite 0.41 / libsqlite3-sys 0.39.1 and ZIP 9.5.1. ZIP entry names are handled through the updated fallible API; declared and actual expanded byte totals are bounded and must agree. `npm audit` reported zero known vulnerabilities. RustSec reassessment reported zero vulnerability entries, with four maintenance warnings and one GLib unsoundness warning remaining in the multi-target lockfile. GLib/GTK is not used by the compiled macOS target; unsupported Linux desktop transitive dependencies remain in the lockfile. Exact findings are in `security/DEPENDENCY-ADVISORIES.json`. This is not an independent security review or proof of absence of vulnerabilities.

Optional ClamAV scanning copies only an explicitly selected ordinary attachment to a restricted temporary file, executes a known local scanner path with bounded time/output/expansion limits, and records a receipt bound to the attachment hash. Signatures and engine installation/updates are user-managed. No automatic upload, deletion or execution of attachments occurs. No scan was actually run during development. No-known-threat is not a safety guarantee.

Reviewed retention can remove disposable public caches, embeddings and old timeline revisions while retaining the latest revision per entity. Current records, original attachments, vault entries and sync tombstones/journal are preserved. Cleanup therefore does not claim to erase all historic plaintext from causal sync data, backups or OS snapshots. Aggregate diagnostic export excludes titles, record contents, IDs, paths, destinations, credentials, prompts and raw errors; it never uploads automatically.
