# Final Mac DMG acceptance audit

The `5bde681` delivery was tested from the actual DMG, not a separately compiled QA app. The audit verifies its checksum, mounted resource signature and Applications shortcut, copies the app unchanged, confirms the executable hash, then launches that executable with a fresh development-only `--qa-profile` namespace. An early guard refuses to open any database if the resolved profile path differs from the requested QA identity. Normal launches keep the standard profile. Release builds reject QA-profile arguments.

At functional source `5bde681`, all 35 enabled public starters fetched 798 real records with no login, API keys, personal records or account flags. Both Apple storefronts have 50 correctly ranked entries. Every card had a usable title, source identity and public link. The process was terminated/relaunched: exact cached payloads and an explicitly disabled refresh policy survived. This is process/cache acceptance, not a claim that every UI control was clicked.

The first actual-DMG audit failed on blank Bluesky cards containing embedded articles/media. The parser now retains attached publisher titles/descriptions/URLs, media alt text and quoted text; genuinely textless posts receive an attributed “no text supplied” caption. Tests save such a card through native validation. The original failed evidence is retained, with a superseding successful result.

Other repaired bugs: apps/tools were missing from the source editor; provider changes assigned the wrong content type; explicit storefront regions could be overridden by matching words in descriptions; identical Apple IDs across countries shared selection/read state; and Apple feed endpoint/slug changes could duplicate saved apps. The editor/filter/state/canonical-link regressions now pass.

Data-driven frontend checks used the DMG’s live cache: all seven topic groups, nine content kinds and fourteen providers were populated; US/India app filters each returned exactly 50, distinct cards had unique IDs, read/dismissed/saved filters and sort modes retained the expected records, and cited documents preserved original URLs/provenance. Native-core acceptance then saved all 798 public entries, created fourteen cited provider documents and a workspace with twenty-eight links, and verified task month-end recurrence, original attachment bytes/index/trash/restore, FTS/reopen, plain/encrypted backups, wrong-password rejection and duplicate-safe restore. These are automated data/core checks, not interactive UI checks.

Current limitation: native UI automation was unavailable in this session. Earlier selected native UI evidence remains explicitly tied to its own revision. Video playback, file pickers, all form interactions, assistive technology, OS permissions/notification delivery and optional private services are not newly certified by this audit. No universal all-feature acceptance percentage is claimed.

Public/access limits remain visible: eight starter sources are paused (USDA historical feed, three Reddit feeds, X/LinkedIn, historical patents and unavailable PIB announcements). An explicit live recheck returned historical patent records and a PIB DNS failure. X/LinkedIn/Reddit approved-access limitations are not fixed by requiring an app login. Google, cloud AI, banking, licensed datasets, WebDAV and executable MCP integrations remain optional configured services. No local model/hardware setup, real account, private folder scan, outreach or production release was used.

## Reproduce with developer tools

After building the normal development app and DMG, run:

```sh
python3 scripts/audit-mac-dmg.py
python3 scripts/audit-public-workflows.py
```

The second script uses existing Node/Rust dependencies offline and temporary databases. Outputs stay under `target/final-dmg-audit`; only bounded, source-specific evidence is copied into `docs/testing/final-*`. The installed app itself requires no developer SDK or command-line steps. Ordinary launches do not need a QA flag.

See [HTML status](STATUS-REPORT.html), [use cases](USE-CASE-AUDIT.html), [test report](TEST-REPORT.html), [artifact provenance](BUILD-ARTIFACTS.json) and `testing/final-dmg-native-qa.json`.

Subsequent Poker tile delivery at `0cf4e1d`: frontend build, nine tournament assertions, existing public-cache behavior and installer checksum/mounted-signature checks pass. This newer UI addition has not repeated the full interactive/runtime acceptance above. See `testing/poker-tile-dmg.json`.


Latest poker repair at `50c55de`: Source 50c55de: four native public schedules returned 58 current/upcoming tournaments, 29 US and 0 India. All 62/62 event/provider HTTPS schedule links passed live reads with no reader-origin changes. Venue/address evidence fixes missed US cities and foreign Texas Hold’em false positives; India positive-case regression passes. No live India matches: enabled-provider coverage is not exhaustive. Unknown countries remain Worldwide. No account/key. 123 workspace test executions (121 distinct), 91 frontend assertions and TypeScript/Vite pass. Interactive UI automation unavailable; no click/player acceptance claimed.

Exact 50c55de DMG mounted, strict signature/hash/shortcut verified and copied unchanged. Fresh isolated process fetched 35/35 public sources (801 real entries), no login/key/personal seed/account flag. Valid card IDs/titles/links and 50 US/50 India app chart ranks. Restart retained caches and off policy; owner profile untouched. Interactive UI not retested. SHA-256: `14ed1ab80ba7f38b7d7a95cff05573e239db464e21f115de8fb360ce57e08202`. See `testing/poker-live-dmg-qa.json`; prior audit records retain original revisions.
