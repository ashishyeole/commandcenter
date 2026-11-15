# Portable data formats

## Desktop ZIP schema 4

`manifest.json` identifies `personal-command-center`, schema 4, export timestamp, item count and SHA-256 hash for every other file. Import accepts schemas 1, 2, 3 and 4; a newer schema fails closed. Schema 3 adds validated non-permission preferences and cached feeds to workspace.json. Schema 4 adds ordinary attachment originals and version/index metadata. Older versions reject unsupported schemas.

- `data.json`: notes/tasks/saved items with IDs, timestamps, provenance, revisions and tombstones.
- `items.csv`: UTF-8 quoted table with the original schema-1 headers. Formula-leading fields receive an apostrophe; JSON preserves exact content.
- `notes/<uuid>.md`: readable note titles and bodies.
- `workspace.json`: structured domain records (fields, tags, links, status, provenance) and journal history.
- `workspaces.csv`: readable `id,module,title,status,summary,created_at,updated_at` table.
- `vault.json`: empty unless vault inclusion was explicitly selected in a password-encrypted export. Contains recoverable vault records and base64 original document bytes; no device key is included.

Plain exports exclude the vault. Ordinary notes/workspaces/history can still contain private text, so the UI warns before export. Native imports reject a plaintext archive with vault records. The core format module is an internal serialization component; callers must enforce this encryption boundary.

Selective ordinary exports include dependency closure for linked records/items and their attachment metadata; review the expanded preview rather than assuming only the initial selection is included. Vault inclusion is separately controlled and requires an unlocked vault plus a backup password.

Excluded: credentials, OS-keychain contents, local folder authorizations, device identity settings, notification delivery cache, integration permission settings, embedding vectors and executable binaries. Vault IDs/modification times exist outside ciphertext locally, but titles/content/filenames do not.

Limits: 64 MiB archive/expanded data, 10,000 ordinary items plus 10,000 workspace records, 100,000 history events, 10,005 ZIP entries. Each document is limited to 10 MiB. Large collections need a streaming format in a later version. Unsafe/duplicate archive paths, unknown fields, bad IDs, duplicate entity IDs, unsupported versions and checksum failures are rejected. Entries are not extracted onto arbitrary filesystem paths.

## Encryption envelope

`.pacc`: ASCII `PACC01`, 16-byte random salt, 12-byte random nonce, AES-256-GCM ciphertext/tag. Argon2id v19 derives 32 bytes with 19,456 KiB memory, 2 iterations and parallelism 1. Minimum 12 Unicode characters. Different parameters require a new envelope version. Forgotten passwords cannot be recovered.

Vault database records use a separate random 32-byte key from the OS secure store, per-record random nonce and AEAD additional data bound to the record UUID. Private backup export decrypts records only within the explicit unlocked operation, then protects the archive with the user's backup password. Restore re-encrypts imported private records under the destination device key. No OS key is exported.

## Restore and conflict behavior

Preview validates and counts without mutating the database. Apply performs ordinary records, imported journal events and missing vault rows in one SQLite transaction. Existing local records are never overwritten.

- **Keep local:** skip duplicate/conflicting IDs and add missing records.
- **Separate copies:** assign new IDs to conflicting incoming entities, remap incoming workspace links and relevant history payload IDs, and import those copies. Vault ID collisions are treated conservatively as conflicts.

A preview is held in memory until apply, discard, lock-vault, or another preview; the frontend discards it when leaving Data & privacy. This is manual restoration, not a peer merge algorithm.

Export writes a temporary sibling file, syncs it, atomically persists it, reads it back and validates it before recording `last_backup`. The metadata records verification at that time; it does not imply the file still exists later. It also records scopes, bytes, encryption and vault inclusion, so an older selective export is never described as a current full backup.

## Mobile JSON exchange

Plain JSON: `{ "format": "personal-command-center-mobile", "schema": 1, "items": [...] }`. Desktop and companion apps accept bounded data with stable item IDs. Companions export active ordinary notes/tasks/saved items; desktop exposes a dedicated mobile export. No vault, workspace records, settings or history are transferred. Import is missing-only and cannot reconcile simultaneous edits to existing IDs.

Mobile JSON is readable outside the app and intentionally requires explicit export/import. Choose a private transfer location; this is neither encrypted backup nor continuous synchronization.

## v0.3 configuration and scheduled destinations

Full-scope archives include dashboard/module/interests/quiet-hour/backup-reminder preferences and public feed caches. Importing those is a separate opt-in and missing-only; it never overwrites local configuration. Notifications, critical escalation, jobs, destination permissions, absolute project paths, credentials and OS keys are not restored. Selective exports omit global configuration/caches.

Scheduled encrypted archives use the same schema/encryption as manual backups. Explicit saved permission is required for destination writes, optional unattended vault access, and optional deletion of older verified app-created files. Read-back verification precedes successful-export metadata. Retention compares ciphertext hashes and only touches recorded paths in the chosen folder. See V0.3-DELIVERY.md for recovery-password handling and runtime limits.

Custom recurrence is encoded as `days:N`, `weeks:N` or `months:N`, bounded to 3650 days, 520 weeks or 120 months. Both v0.3 mobile companions accept these strings. Older companions may reject them; upgrade before transferring custom schedules.

## v0.5 documents, mobile vault and cleanup

Ordinary attachment originals are limited to 10 MiB each, with hashes/version metadata and up to 100 KB searchable text. PDF text and PNG/JPEG OCR may be indexed through explicit, separately installed local Poppler/Tesseract tools; extracted text is bounded and may be incomplete/inaccurate. Schema-4 archives include originals and JSON metadata (which may duplicate base64 bytes); the 64 MiB expanded/archive limit still applies to the combined content. Large collections require a future streaming format. Attachment originals now use incremental causal peer synchronization when both attachment and parent categories are explicitly approved. Interrupted files retry from the beginning; committed batches resume by acknowledgement. See ATTACHMENT-SYNC.md. Private vault attachments remain in explicitly encrypted private exports/transfers.

Mobile vault record access uses platform-protected keys and expires on timeout/background. Private archive exchange requires explicit password entry and validates before importing separate copies. Existing private file bytes are retained; mobile add/preview remains incomplete.

Reviewed retention can remove ordinary historical change rows while preserving each entity’s latest row, public caches and embeddings. It intentionally preserves causal sync events/tombstones/heads; old content may remain in that journal, OS snapshots and previous backups. It is not a complete secure-erasure feature. Diagnostic exports contain metadata/counts only.

System file pickers may background and lock mobile vault views. Encrypted transfer bytes can remain in memory across that lock; keys/passwords/plaintext are cleared. After selecting a private archive, unlock again and enter the recovery password before preview/import. No decryption or import occurs solely from file selection.
