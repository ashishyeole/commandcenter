# Public Apple charts

Discover includes US and India Top Free App Store charts automatically, with no login or API key. Existing subscription/off settings are preserved.

The previous `rss.marketingtools.apple.com` host intermittently failed with transport errors; a direct public read reproduced HTTP 502. The chart adapter now reads Apple’s working iTunes RSS feed first (`https://itunes.apple.com/{country}/rss/topfreeapplications/limit=50/json`), with the Marketing Tools feed as a fallback for transport or transient 500/502/503/504 failures. Both are Apple-owned feeds for the same storefront and chart. The old `rss.applemarketingtools.com` alias redirects to the failing host and would not provide an independent path.

Each endpoint has a five-second connection deadline and twelve-second complete-request deadline; at most two requests are made. The hub does not multiply that fallback with its generic retry. Authorization failures, redirects, rate limits and Retry-After responses stop rotation; rate-limit pauses persist in the provider cooldown. Existing cancellation, reset checks and failed-refresh cache retention apply. No account connection, private data or new package is needed.

Both JSON schemas normalize to the existing app IDs, original App Store links and publisher/release fields. Charts retain the requested storefront and original order. Empty/malformed charts, mismatched feed identities/links, duplicate IDs and more than 50 rows are rejected rather than producing partial or renumbered results. Each cached entry carries the actual feed URL, storefront, publisher update time and any fallback use/errors. Rankings are chart snapshots, not download/revenue or growth estimates.

Verification: five regression tests include actual isolated HTTP gateway fallback, refusal handling and persisted Retry-After pauses. An explicitly run native live test refreshes both starter streams through the same hub/cache pipeline and checks 50 ranked entries per storefront, metadata, health and exact cached content. The installed Windows audit additionally checks these two charts in the exact packaged EXE. Exact source revisions and outcomes are recorded in the HTML report and testing evidence.

Apple references: [RSS information and feed generator](https://www.apple.com/au/rss/), [Marketing Tools feed builder](https://rss.marketingtools.apple.com/). Public feed availability and chart contents can change; failed reads retain the prior cache and show an error.

Delivery evidence at source `36350b0`: native live test returned 50 US/50 India entries; fresh Mac and exact installed Windows each fetched 35/35 public sources with 800 real entries, zero failures, and no login/key. Windows workflow `44039054861` verifies the installed charts and installer hash. Mac/Windows checks and packages passed; the local Mac DMG passed checksum, mounted signature and delivery-hash checks. See the [HTML report](STATUS-REPORT.html) and `testing/apple-*.json`.
