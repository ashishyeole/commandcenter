# Public-calendar recurrence

The local calendar importer expands a bounded subset of [RFC 5545 recurrence](https://www.rfc-editor.org/rfc/rfc5545.html#section-3.3.10). This processes publisher data; it does not discover all tournaments, confirm a schedule, infer guarantees, register for an event or send anything.

Supported combinations:

- All-day dates, UTC timestamps, IANA timezone timestamps, and floating local times.
- DAILY, WEEKLY, MONTHLY and YEARLY with INTERVAL and exactly one of COUNT or UNTIL.
- BYDAY: plain weekdays; month-relative ordinal weekdays for MONTHLY, and YEARLY with explicit BYMONTH.
- BYMONTHDAY: positive or negative month days, except with WEEKLY. YEARLY filters require explicit BYMONTH.
- BYMONTH and WKST. Other rule keys are rejected for expansion.
- Parsed EXDATE and RDATE values, including multiple comma-separated values. Exclusions win; additions are deduplicated. COUNT is applied before exclusions, and RDATE is independent of COUNT/UNTIL.

Monthly dates that do not exist are skipped, rather than clamped to month end. Leap-day yearly series skip non-leap years. IANA series preserve local wall-clock time across DST; invalid or ambiguous local occurrences are omitted and do not consume COUNT. Explicit DTEND duration is preserved as elapsed time for zoned events and local duration for floating/all-day events. All-day ends remain exclusive. Existing expanded all-day IDs are preserved so watched events continue matching after refresh.

Bounds: at most 366 occurrences per series, a 36,600-day scan, 1,000 events/expanded instances per calendar and a 2 MiB calendar response. An incomplete expansion is discarded and the original recurring declaration remains visible for source review. DTSTART must match the supported rule. Zoned UNTIL must be UTC; floating/all-day UNTIL must match its value type.

Remaining cases: BYSETPOS, time-of-day filters, year/week-number filters, EXRULE, DURATION, custom VTIMEZONE definitions, unbounded rules, and series containing RECURRENCE-ID overrides remain unexpanded. Malformed/duplicate rules and invalid exception/end-date declarations also remain unexpanded. This is deliberately narrower than a complete calendar engine. Publisher dates should still be verified before travel or registration. Specific registration/satellite alert rules remain separate backlog work.

Seven synthetic parser integration tests cover monthly invalid dates, negative month days, ordinal weekdays, leap years, interval weeks, DST, durations, exclusions/additions, inclusive UNTIL, stable IDs and unsupported-rule fallback. Three earlier recurrence tests remain. No live calendar was accessed; native calendar UI and real provider acceptance are not claimed.

## Additional bounded rules

BYSETPOS, positive DURATION (whole calendar days keep wall time across DST; time components use exact seconds), EXRULE, and individual RECURRENCE-ID overrides are supported. A rule without COUNT/UNTIL uses a rolling 366-day window ending today + 365 days; this is a preview bound, not an invented series end. Cancelled individual overrides are retained, including those without DTSTART or SUMMARY. RANGE overrides and unsupported rule components remain explicitly unexpanded. Existing scan/candidate/output bounds still apply.

Calendar watches now include publisher descriptions, end dates, registration status/URL, categories and related-event identifiers as well as date/status/location. This catches registration and satellite announcements only when the publisher actually changes those fields. Combined-feed tracked events also create review alerts for changed publisher details on refresh, retaining saved plans. Disappearance from a bounded page never implies cancellation. No registration, guarantee, open-seat availability or independent source authority is inferred.
