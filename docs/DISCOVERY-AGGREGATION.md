# One information feed

The core product is now topic-first aggregation. The app starts in Discover, combining models, repositories, videos, papers, news, public social posts and current/upcoming events. The former single-provider search remains under **Single-source search**.

New installations configure the public starter sources and start fetching immediately, without an app account or API key. Automatic refresh defaults to 30 minutes while open and can be turned off or changed. Existing configured/off/restored policies are preserved. Subsequent visits use the local cache; **Refresh all feeds** updates enabled sources. A transient public transport failure is retried once; auth/quota/private-source failures are not retried. One failed provider does not hide other providers or erase its last successful cache.

## Included starter coverage

| Topic | Live sources |
| --- | --- |
| AI and models | Hugging Face trending/download rankings, Ollama catalogue, AI community news, Hugging Face videos |
| Code and libraries | GitHub top public repositories and recently created repositories |
| Robotics | GitHub libraries, Hugging Face robotics models, arXiv papers, IEEE Spectrum, MIT CSAIL videos, Bluesky and Mastodon |
| Agriculture | AgFunderNews, AGDAILY, agri-tech discussions and posts, John Deere videos; AgriTech India, World Ag Expo and AGRITECHNICA dates |
| Poker | PokerNews, PokerStars videos, WPT, WSOP, PokerStars Live and CardPlayer’s multi-organizer calendar |
| Learning | MIT OpenCourseWare and Stanford Online channel uploads |

The USDA research RSS remains available but is paused in the starter set because its latest listed material was old during live verification. Source dates and fetched times have different meanings. A fresh request does not make an old publication new.

## Filters and source subscriptions

The combined view filters by topic, provider, content type, US/India/worldwide coverage, reported publication/update window and reading/saved status. It offers balanced topic/source order, newest reported date and upcoming event order. Rankings stay attributable to the provider; the app does not add stars, downloads and social likes into an invented popularity score. Regions use configured source coverage and explicit venue/location text; unknown locations remain worldwide.

In **Sources & filters**, edit or add subscriptions:

- GitHub: native query operators, stars/forks/updated ranking, creation window, programming language and license.
- Hugging Face: model-name search, trending/download/like/update ranking, model task, library/language tag and license. Task uses `pipeline_tag`; multiple tag filters use repeated `filter` parameters. Live checks caught and corrected the earlier comma-separated filter behavior.
- Public YouTube: add an `@handle`, official channel URL or channel ID. Up to 15 recent uploads come from the channel feed, including Shorts. This works without a key and is separate from a personalized home feed. Broad keyword video search needs the optional YouTube Data API key; API search supports order, upload window, duration and region.
- Publisher RSS/Atom, iCalendar and supported public event pages can be added by HTTPS URL. Event ingestion handles structured Event metadata plus the tested organizer layouts. Timed event cards show UTC explicitly and retain original timestamps; venue-local dates should be checked at the organizer. Ended/cancelled events are hidden; iCalendar all-day end dates are converted from exclusive to inclusive dates.
- Public Bluesky search and Mastodon hashtag reads work without account credentials. Posts remain publisher/user statements.

Saving source setup makes no request. Pausing a source retains its cache and removes it from normal feed browsing/refreshes. **Include paused source caches** permits offline review. Source requests use at most three concurrent groups, shared provider cooldowns, bounded responses, deadlines and cancellation. Public feed URLs resolve only to public internet addresses. Redirected publisher/calendar/catalogue requests retain their HTTPS and destination checks.

## Save, learn and start work

Save individual items or select up to 100 at once. A saved URL is reused rather than creating another library entry. Create an editable Markdown research document with descriptions, source URLs, observed metrics, dates and provenance. Large selections split into documents within the ordinary record size limit. Uploaded files attach to these documents or workspaces; the global Documents view also has an owner picker and document creation. Original files stay unchanged.

Add discoveries to a new project or an existing workspace. Sources remain linked, and a project started from one GitHub repository gets its repository URL. Track a dated event into Poker Hub or Agri-tech, preserving source dates and venue text. These are local records; registration, booking and gambling transactions are not performed.

Read cached details inside the app. **Open original in app / Watch in app** uses one separate source-reader window with no native app permissions, public HTTPS destination checks and same-host navigation. File downloads in this reader are disabled. Publisher paywalls and video playback rules still apply. Original links can also be opened normally.

Portable standard and streaming backups now accommodate the aggregated caches and source setup. Restored subscriptions remain disabled until reviewed; automatic-refresh permission, API keys and account authorization are excluded. Existing receiving configuration is kept. Watch thresholds use the newest cached observations, including hub feeds.

## Access boundaries

| Source | Current dependency |
| --- | --- |
| GitHub / Hugging Face / Ollama / papers / news / calendars | Internet and provider availability; public metadata needs no account or model runtime |
| Public YouTube channels | Internet only; broad search needs an optional API key |
| Bluesky / Mastodon public feeds | Internet and public API availability |
| Reddit | Public RSS returned 403/rate limits from the app during live testing, so starter subscriptions are paused. Optional official reads require an approved installed-app client and read-authorized refresh token. That credential path is implemented but not live-tested. |
| X | Recent-search connector needs developer API access and a bearer token. User has no developer access; live X reads remain blocked. |
| LinkedIn | No open personal home-feed API. Approved community/posts permissions are restricted and do not equate to general home-feed access. Live personal-feed aggregation remains unavailable. |

X, LinkedIn, Reddit and other public links can still be added to the combined feed with a user-supplied title/summary, or imported from a bounded JSON file. Imported items are labeled as imports, not live provider results. The schema is shown in the app. No account passwords are collected. Optional tokens remain in the OS secure store and require a device-local configured flag before use.

Official references: [Hugging Face client filters](https://huggingface.co/docs/huggingface_hub/package_reference/hf_api), [YouTube search parameters](https://developers.google.com/youtube/v3/docs/search/list), [X recent search](https://docs.x.com/x-api/posts/search/integrate/build-a-query), [LinkedIn access restrictions](https://learn.microsoft.com/en-us/linkedin/marketing/community-management/community-management-overview?view=li-lms-2026-08).

No model weights, local inference engine, robot simulator or hardware are downloaded or run. Public discovery sends only the configured queries/URLs, not local documents or workspace content. The assistant remains optional online AI configured separately.

## Verification

See `testing/discovery-live-sources.json`, `testing/reddit-live-access.json`, `testing/discovery-native-qa.json` and `TEST-REPORT.html` for actual runs and limitations. Public source results can change after the recorded check. Live credentialed social/account paths and every-feature acceptance are not claimed.

Latest verification: 2026-11-16 native public run refreshed **30/30 enabled sources with zero failures**; 611 source items were observed in the app. Both native live filter checks returned 30 matching records (GitHub Python/MIT; Hugging Face text-generation/transformers/apache-2.0). Public feed contents/counts change over time. Selected native save/document/file/workspace/event/India-filter/video/import/restart checks are recorded in `testing/discovery-native-qa.json`. Public Stanford video playback worked without sign-in. X/LinkedIn live feeds remain unavailable; Reddit starter feeds remain paused after recorded public access refusals.

Latest no-account integration audit (source 2cb141d): **35/35 enabled public sources**, 803 raw source entries; native GitHub and Hugging Face filter checks each matched 30 records. Research-paper, US/India app-chart and public MCP starters are included. Domain workspaces show related real public caches alongside personal records. Keyless single-channel refresh feeds creator research. MCP catalogue requests use latest-version filtering, distinct version identities and matching source URLs, following the [official registry API](https://github.com/modelcontextprotocol/registry/blob/main/docs/reference/api/official-registry-api.md); nested package/remote metadata is readable JSON.

Fresh isolated Mac UI observed 791 combined entries after recovering an Apple US transport error. A robotics search returned 150 live results across five providers. Save, two-source cited document, linked project, native selected-file upload/preview, tracked event, linked-task completion, keyless channel refresh and research drafts were exercised. Earlier video playback evidence remains revision-specific. Public OSM lookup is separate from these feeds: the chosen public service failed and alternate public-service reads timed out. These are recorded failures, not substituted sample listings. See [all use cases and dependencies](USE-CASE-AUDIT.html).

Final delivery evidence: functional source `2cb141d` passed both Mac/Windows checks (`43997314188`) and both package jobs (`43997312953`). The local Mac DMG passes checksum/read-only mount/version/strict ad-hoc signature/shortcut/delivery-hash checks. The Windows EXE passes silent install/native-window/uninstall. An additional installed-Windows audit (`43999649407`, QA harness at `78f403e`) reads only its fresh runner profile and proves **35/35 public sources, 799 live items, no app login/API keys/account flags or seeded personal records**. It uses the exact SHA-256-matched `2cb141d` installer; it does not rebuild or alter app functionality. The initial independent runner yielded 34/35 and is retained as `testing/online-windows-bootstrap-initial.json`; final per-provider health has zero failures. This verifies native public startup/data population, not full Windows feature UI. Final Mac Stop/retry recovered 40/40 (876 combined records including five added public-search sources).


## Public business lookup repair · November 17, 2026

Functional source `061b3b9` restores default no-account lookup with supported public Overpass services; `6442ed4` adds a bounded same-provider transient retry. The old failing endpoint is no longer the default. Bounding-box queries avoid global around scans; native code filters reported coordinates/way centres to the requested radius and rejects unlocated, malformed or partial results. At most 50 actual listings are shown with attribution, source, fetch time and approximate distance. Provider pool is disclosed before approval; custom endpoints contact only that endpoint. Access/429 refusals and Retry-After stop/pause requests. Cancellation/reset cannot publish results; cache context, empty/untracked states, busy controls and duplicate-safe CRM saves were completed. No account, API key, local model or owner credential is required.

Nine new native regressions pass; full workspace 111 target/test executions (109 distinct), frontend build and 74 assertions pass. Native live reads returned 50 Austin and 50 Bengaluru listings and verified caching/deduplication. Selected Mac UI default search/real lead save/cache/restart/Stop recovery passed. Global India GUI requests remained intermittently unavailable (VK Maps 504 / Private.coffee transport), so that provider limitation stays visible; prior cache and saved lead were preserved, with no sample substitution. This closes the identified code-only business gap, not public-service uptime or all-platform acceptance. See `BUSINESS-DISCOVERY.md`, `testing/business-native-qa.json` and `testing/business-live-sources.json`. Other remaining dependencies are optional provider credentials/data and target-device/voice/push/accessibility verification; no new owner setup is needed for local organization/public business search.


## Final global provider repair · November 17, 2026

Functional source `50bffb5` adds the documented public Photon OSM category/radius endpoint as the global primary, with scoped Overpass fallback. This resolves the global GUI failures left by overloaded Overpass-only providers. No account, API key, Nominatim service or local installation is required. Native validation preserves OSM IDs, category, source links and in-radius coordinates; missing website metadata is explicitly unknown. Both public-city native tests returned 50 records in about two seconds total. Final native Mac UI fetched 50 Bengaluru results, saved The Merak Brewhouse, retained the earlier 24 Diner record, showed 49 untracked results and reused the unchanged cache timestamp; provider disclosure identified Photon. Earlier failures remain revision-specific evidence. Ten new business regressions and the full 112-test-execution workspace suite pass (110 distinct), plus 74 frontend assertions and frontend/native builds. No identified remaining code-only gap in the clarified monitoring/local organization scope; optional credentials/data, device/voice/push/accessibility acceptance and provider uptime remain separate. See `BUSINESS-DISCOVERY.md` and the exact-source evidence.


Apple chart repair — 2026-11-17, functional source `36350b0`: reproduced old primary HTTP 502, changed public Top Free charts to Apple iTunes RSS with bounded Marketing Tools fallback. Same storefront/rank/app identity, source URL/update metadata, rate-limit pauses, strict parsing and existing cache/cancel/reset behavior are retained. Five new regressions and full 117-test workspace pass. Explicit native live pipeline returned 50 US and 50 India rows; fresh isolated Mac startup fetched 35/35 public streams, displayed real ranked cards and original feed details, and persisted 50 rows per chart. No login/key, private account or normal profile used. Earlier failed Windows chart read remains historical; matching installer evidence is tracked separately. See `APPLE-CHARTS.md` and `testing/apple-*.json`.


Final Mac DMG audit — 2026-11-17, functional source `5bde681`: added explicit development QA-profile isolation with a path guard, so the actual DMG executable/assets can be copied and tested unchanged without touching the normal profile. The first exact-artifact audit caught blank Bluesky article/media cards; fixed attributed titles and preserved embed details, with native saved-record regressions. Also repaired apps/tools source editing, provider kind assignment, declared-region precedence, cross-storefront app selection/read IDs and Apple endpoint-alias saved duplicates. Full workspace: 120 executions / 118 distinct; frontend: 79 assertions. Native live public/filter/business/chart/scholarly checks ran explicitly; PIB DNS still fails and patents remain historical. Final actual DMG passed 35/35 public refreshes (798 entries), title/link/ID/storefront checks, exact binary/signature/hash checks and process restart with cache/off policy retained. Real-cache frontend behavior and native-core 798-save / fourteen-doc / linked-workspace/task/attachment/backup/FTS/reopen acceptance pass. Native UI automation was unavailable, so no current every-button/form/dialog/player/OS-permission acceptance is claimed. Earlier UI evidence retains its exact revision. See DMG-ACCEPTANCE.md and testing/final-*.
