# One information feed

The core product is now topic-first aggregation. The app starts in Discover, combining models, repositories, videos, papers, news, public social posts and current/upcoming events. The former single-provider search remains under **Single-source search**.

Open the app and choose **Load my feeds** once. Subsequent visits use the local cache; **Refresh all feeds** updates the enabled sources. One failed provider does not hide other providers or erase its last successful cache. Automatic refresh is optional (30 minutes, one hour or three hours) while the app is open.

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
