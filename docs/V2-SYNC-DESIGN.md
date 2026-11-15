# V2 peer synchronization — implementation and boundaries (v0.4)

The repository now implements optional same-LAN peer synchronization. No service has been started during development, and no runtime or security acceptance has been run: the user requested no tests. Source implementation and compilation are not a security certification.

## Authentication and transport

Desktop discovery uses mDNS `_pcc-sync._tcp.local.`. iOS uses Bonjour and Android uses NSD. Discovery exposes a random instance and public-key fingerprint hint, never proof of trust. Manual private IPv4 address/port entry is available when multicast fails. Desktop listeners are explicitly enabled per app session; phones can initiate connections or explicitly enable a foreground receiver, with native Bonjour/NSD advertising. Mobile receiving stops on screen exit or app background. Guest Wi-Fi, client isolation and firewalls can prevent reachability.

Pairing uses a five-minute, one-use invitation containing a 256-bit secret, pinned inviter public key and port. Noise XXpsk3 authenticates the invitation exchange; the inviting device displays the comparison code, peer identity and category intersection for approval. Subsequent connections use Noise KK with pinned static keys. Cipher suite: X25519 / ChaChaPoly / SHA256. Private keys and trust live in the OS secure store; mobile trust is protected by Keychain or an Android Keystore AES-GCM key. Ordinary data categories never imply vault access.

The wire implementation bounds preludes, frames, messages, connection concurrency and timeouts. A separate prologue distinguishes private transfers from ordinary sync. No HTTP endpoint or globally routable public-endpoint mode exists. Explicit private-mesh mode additionally permits the 100.64.0.0/10 range used by private virtual networks; ordinary private IPv4 VPN routes also work. Code references: `apps/desktop/src-tauri/src/peer_sync.rs`, `sync_wire.rs`, `crates/mobile-peer/src/lib.rs`. Library APIs: [Snow builder](https://docs.rs/snow/0.10.0/snow/struct.Builder.html), [mdns-sd](https://docs.rs/mdns-sd/0.21.7/mdns_sd/).

## Replication and conflicts

Storage migration 4 adds immutable events, vector-clock heads, seen-event digests, conflict records and per-peer cursors. Local edits are captured before exchange. Batches scan at most 64 events and approximately 3 MiB; sessions run at most 32 rounds before a later session resumes. Selected scopes are fixed when paired. Receiver transactions commit data, event deduplication and cursor together before acknowledgement. Offline edits remain local until a trusted peer becomes reachable.

Causal successors apply; causally older events are ignored. Concurrent independent metadata changes and set changes can merge. Divergent text and delete-versus-independent-edit remain visible conflicts. Choosing local, incoming or an edited version creates a new causal resolution; original events remain in history. The desktop offers JSON merge review; mobile offers local/incoming resolution. Vector clocks support up to 64 historical device identities per entity. Unresolved conflicts are bounded at 2,000; journal compaction is not yet implemented.

Local folder bindings, integration credentials, caches, installed models and repositories are not replication entities. User-authored text and explicitly selected project/session summaries are shared, so users should review category scope before pairing.

## Mobile and reminders

Both native companions call the shared Rust causal store through C ABI/JNI. Legacy mobile databases migrate missing records once and remain on disk. Project/mission/session summaries, plan approvals and conflict decisions use the same event protocol. Mobile approvals are requests for desktop review, not authorization to execute arbitrary tools. Mobile shows last exchange and queued journal entries. The UI preserves a working endpoint after a successful sync.

Automatic desktop polling is opt-in while the listener is enabled. The lower device UUID initiates to avoid simultaneous scheduled sessions. Both desktops should enable automatic polling. Task-scoped peers exchange up to 128 notification delivery receipts. A desktop suppresses an occurrence only after a successful exchange provides a matching receipt and the peer has acknowledged its current task version. Mere peer reachability never suppresses an alert. Phones can receive foreground sync directly from each other. They do not claim continuous background sync or remote notification delivery. Offline devices can still produce duplicate reminders.

## Private categories and recovery

Private document/finance/subscription/company categories have a separate explicit copy workflow between desktops. Both vaults must be unlocked; the recipient approves each offered category/count for that transfer. Plaintext is not written to the ordinary journal. Transfers are bounded to 50 records and 64 MiB, re-encrypted with the recipient vault key, and committed atomically. Divergent IDs produce deterministic, labeled incoming copies instead of overwriting. This is a manual category transfer, not automatic causal vault replication; mobile vault transfer remains absent.

Revocation removes the local trust allowlist before secure-store cleanup. Active exchange checks trust between messages. Stopping sync cancels connections. Explicit key replacement stops listening, revokes all local pairings and generates a new key. Other devices must revoke the old pairing before pairing again. Revocation cannot erase copies already held by another device. Portable backups do not restore pairing trust or device private keys.

## Backup and optional networking

Encrypted scheduled backups support local/mounted folders and explicit HTTPS WebDAV destinations. Recovery passwords and WebDAV app credentials stay in OS secure storage. Archives are encrypted before upload; redirects are refused. Read-back decrypt/manifest/hash verification precedes success reporting. Retention deletes only recorded, unchanged app-owned archives; WebDAV additionally requires a strong ETag and conditional deletion. Missing server support preserves the archive and reports a warning. Backups run only while the desktop app is open.

An optional existing private mesh/VPN route is implemented: enable it in both apps, configure the VPN independently, and enter the peer’s virtual IPv4 address and current port. The app does not install or control a VPN. Built-in internet relay/NAT traversal and continuous mobile background delivery remain unimplemented alternatives. No infrastructure or provider account has been provisioned. Independent security review, cross-device interoperability, disconnect/conflict/recovery exercises and signed distribution remain open.
