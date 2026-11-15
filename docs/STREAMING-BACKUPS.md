# Large encrypted streaming backups

Data & privacy offers `.paccs` backups alongside the existing ZIP/PACC01 format. PCCSTR01 processes individual records rather than collecting the full archive in RAM. Current bounds: 8 GiB encrypted file, one million data/config frames, 24 MiB plaintext per frame and existing per-attachment 10 MiB limits. The identity set used for duplicate checking grows with record count; payload buffers are bounded per frame. Large imports hold the database transaction/lock and can be cancelled.

Includes notes/tasks/saved records, structured workspaces and their explicit links, original attachments, full ordinary timeline history, safe display preferences, public feed caches and research collection definitions. Optional private records require an unlocked source vault and are sealed with the receiving device’s key on import. Device keys, secure-store integration credentials, folder grants, account connections, OS schedules, sync trust/cursors and other permissions are excluded. Ordinary record text and URLs are retained as entered; the exporter does not classify or redact secrets embedded in free-form content. This is a desktop format; mobile companions retain their existing exchange and vault backup formats.

## Format and integrity

- Header: eight ASCII bytes `PCCSTR01`, 16 random salt bytes, four random nonce-prefix bytes.
- Password key: 32 bytes from Argon2id v19 with fixed memory 19,456 KiB, two iterations, one lane and a 32-byte output (matching current standard-backup defaults). New exports require at least 12 Unicode code points.
- Each frame: unsigned big-endian 32-bit ciphertext length followed by AES-256-GCM ciphertext and tag. Nonce is the four-byte archive prefix followed by the unsigned big-endian 64-bit frame index. The full 28-byte header is authenticated as additional data.
- Plaintext is UTF-8 JSON with adjacent `kind` and `value` fields. Recognized kinds: Item, Record, Attachment, History, Feed, Preferences, Collections, Vault, End. Unknown versions/kinds/fields are rejected.
- The final encrypted End frame contains the count and SHA-256 digest of the concatenated exact plaintext bytes of preceding frames. Truncation, altered ordering/content, a missing footer or trailing bytes fail verification.
- Native preview makes an encrypted temporary copy of the selected file. Its SHA-256 is bound to the review. Import recomputes the encrypted-file digest while reading and checks it before committing. Temporary plaintext archive files are never created.

## Restore policy

Preview verifies every frame, schema/value validation and the terminal manifest before presenting counts. New record IDs are imported in one SQLite transaction; existing IDs keep the local record, including conflicts. Existing configuration is retained. A wrong password, invalid frame, missing owner, corrupt footer, changed review checksum or cancellation before commit rolls the transaction back. Streaming import does not offer per-record conflict copies; use standard import for smaller archives or retain the incoming archive for later comparison.

Export uses a consistent database read transaction, writes an encrypted temporary file beside the destination, verifies it, then publishes it atomically. Reset epochs prevent stale previews/exports from being published into the new profile state. Background source or cloud jobs are not restored by importing a backup.

## Verification

Repository tests cover an actual file larger than 64 MiB, import idempotence/local conflict preservation, attachments/links, receiver-key vault encryption, permission exclusion, corrupt/truncated/trailing data, wrong password/review digest, authenticated duplicate IDs, unsupported format versions and interrupted reads. Native UI smoke coverage is recorded separately in `testing/`.

This is a new application format with documented bounds, not a claim of independent cryptographic review or unlimited library size. Known standard formats remain supported; future formats fail explicitly until a compatible migration is implemented.

Streaming export is currently manual. The scheduled local/WebDAV backup adapter retains its existing standard archive limits. Per-record conflict-copy merging for large streams and a scheduled streaming destination remain future work.
