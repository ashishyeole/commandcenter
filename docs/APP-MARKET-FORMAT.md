# Licensed app-market import format

The desktop imports UTF-8 JSON files up to 1 MiB, with 1–250 app observations and no unknown fields. It never retrieves a proprietary dataset or scrapes a store automatically. Obtain permission and convert the provider's licensed export to the documented schema yourself. The example below describes the format, **not actual market data**; replace all example metadata with your evidence. HTTPS source URLs must have no credentials or fragment (use a stable citation or store URL; query parameters such as a Play Store app ID are allowed, but never include API keys or access tokens). Dates must not be in the future.

```json
{
  "schema": 1,
  "provider": "Your licensed dataset provider",
  "source_url": "https://example.com/dataset",
  "license_note": "Your actual permitted use and source/license reference",
  "market": "US",
  "period_start": "2026-10-01",
  "period_end": "2026-10-31",
  "observed_at": "2026-11-01T12:00:00Z",
  "apps": [{
    "id": "provider-stable-app-id",
    "platform": "android",
    "name": "Format example only",
    "category": "Productivity",
    "store_url": "https://example.com/app",
    "features": ["Feature observed in the cited evidence"],
    "monetization": "Record observed pricing, subscriptions and date; do not infer revenue",
    "ux_notes": "Your attributed UX observations",
    "downloads": null,
    "revenue_minor": null,
    "currency": "USD",
    "measurement_basis": "manual"
  }]
}
```

Platforms: `ios`, `android`. Basis: `provider_reported`, `estimated`, `manual`. None means independently verified by the app. Null metrics are unknown; zero means the dataset explicitly reports zero. Integer metrics cannot exceed JavaScript's safe-integer range. `revenue_minor` uses two-decimal minor units; normalize to that convention before import and never mix currencies. Observation identity is platform plus provider-stable ID. Duplicate identities within a snapshot fail validation.

Up to eight snapshots are cached, with the latest import first. Importing the same provider/market/observation timestamp replaces that cached snapshot. Compare up to four competitors. Period changes require the same provider, market, app identity, metric basis and equal-length periods, and are labelled as differences between observations, not forecasts or verified growth. Overlapping periods or changes in provider methodology still require human review. Save a reviewed comparison note for portable storage; caches can be removed by the retention control. Originals remain unchanged.
