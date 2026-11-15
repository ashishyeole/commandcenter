# Attachment sync

Ordinary attachments now use the same mutually authenticated Noise channel and causal journal as notes and projects, across desktop, iOS and Android. Private vault files remain in the separate explicitly approved encrypted vault-transfer workflow.

## Enable

1. Update both devices to a build that includes attachment sync.
2. Pair with **Attached files / attachment** selected on both devices AND select the parent categories, such as Notes or Projects. Existing pairing grants do not expand automatically: revoke and pair again to add the scope. Previously shared copies are not erased by revocation.
3. Start the receiving listener and choose **Sync now**. Mobile receiving requires the foreground. Desktop automatic sync can use the same approved categories.
4. On the companion, open **Synced attachments** to search filename/indexed text and explicitly export an original through the OS destination picker. Exported originals are plaintext ordinary documents; no unknown file type is executed inside the companion.

## Behavior and boundaries

- Originals retain IDs, SHA-256, owner, filename, version predecessor, creation time, extracted text and trash state. A file's content, owner, predecessor and creation identity cannot silently change; new file versions use new IDs.
- Receiving checks the checksum, 10 MiB decoded-file limit, approved owner category, entity ID ownership and 1,000-version workspace limit before commit. A rejected batch does not advance the incoming cursor.
- One attachment event travels per batch, after its parent records. Noise frames remain bounded to 60 KB of plaintext; the authenticated message cap is 32 MiB to include a 10 MiB file and its causal base encoded in JSON. Ordinary batches retain their smaller sender bound.
- Acknowledged batches resume after disconnection. An interrupted file retries from its beginning; there is no byte-offset resume or persistent partially received file. Device health shows committed changes and journal acknowledgement progress, not a byte-level progress bar.
- Trash/restore/index changes follow the causal merge/conflict rules. Concurrent delete-versus-edit is reviewable; conflicting extracted text is preserved for a choice. Conflict previews omit file bytes and extracted text; choose local or incoming. Editing attachment JSON in conflict resolution is intentionally rejected.
- Files are stored in local SQLite and retained in causal history. This favors recoverability but duplicates encoded contents in the journal; it is not a streaming large-file system or deduplicated blob store. Existing archive size limits remain. Prefer modest personal collections.
- Mobile notification transport never carries attachments. Sharing files requires peer reachability and explicit pairing scopes; native push does not bypass those scopes or trigger remote file execution.

Compiled on macOS, shared mobile Rust, iOS Simulator and Android. Automated repository sync/conflict/permission/retry and encrypted localhost transport tests now pass; see TEST-REPORT.html. Real paired-device UI, network-loss and device-loss acceptance remain outstanding.
