# Optional Google background refresh

Off by default. After configuring and connecting the existing read-only Google account, Settings offers independent inbox-header/calendar scope choices, a 15-minute to 24-hour interval, and explicit recurring-read approval. Changing account credentials/scopes disables the saved background permission. No other email/calendar providers are added.

While the app is open, the poller reuses the existing bounded Google reader: first 20 inbox headers and/or first 100 primary-calendar events in the next 30 days. It reads no message bodies or attachments, never marks mail read, and performs no calendar writes. Each background read has a 90-second deadline. Foreground Google operations take priority; no after-quit collector is installed.

Snapshots are serialized in a zeroizing in-memory buffer for at most 30 minutes. A configuration ID and profile epoch bind results to the reviewed permission. Stopping/changing permission cancels an active Google request and clears the snapshot; disconnect, account replacement and local reset also clear it. Expired or stale-profile/configuration results are never returned. The periodic collector releases expired buffers even if nobody opens Inbox. No account content is written to ordinary SQLite, sync, search, exports or AI context. Only configuration and redacted job health are stored locally.

Inbox can show the latest approved memory snapshot after view consent; manual pagination remains separate. The existing Inbox view clears displayed results when hidden or after five minutes. Saved permission controls future background reads independently of that temporary view. Users can still explicitly copy individual manual results into the encrypted vault.

Two synthetic tests cover required consent/scopes and expiry/reset/replacement invalidation. No Google account was connected or read during testing; live provider acceptance requires the owner's OAuth client and sign-in.
