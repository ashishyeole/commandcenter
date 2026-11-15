# Optional hosted services

This V3 service is optional. It never opens a desktop database and cannot decrypt an archive. Native local capture, vaults, search, export and LAN sync have no dependency on it. No service has been deployed or exercised during development.

Implemented: token-to-tenant authorization, owner/member/reader roles, per-job ownership/revisions, scheduled public-source collectors, opaque encrypted archive mailboxes, bounded audit history, owner export/reset, moderated declarative extension catalog, publisher withdrawal, and configurable hosted-only license/seat policies. Public registration, identity federation, billing-provider automation and production security acceptance remain unfinished. Native APNs/FCM provider source is implemented but not configured or exercised. The desktop has connection, collector, relay, publishing and moderation controls.

## Operator configuration

Use independently generated random access tokens of at least 32 bytes. Store only their SHA-256 hex digests in `PCC_PRINCIPALS_JSON`, loaded through an OS environment/secret manager. Issue each member a distinct token through a secure channel. Do not put real tokens, principal JSON or secrets in this repository. Each principal has:

```json
{"tenant":"team-id","subject":"member-id","role":"owner","token_sha256":"64_LOWERCASE_HEX_CHARACTERS","expires_at":1893456000,"catalog_moderator":false}
```

The value is an array of principals. Accepted roles: owner, member, reader, scheduler. An access key identifies exactly one tenant and subject. Owner can administer its tenant, member can manage its own collectors and publications, reader can read shared public-source results and receive/acknowledge its own encrypted envelopes. Only a separately configured owner with `catalog_moderator: true` can approve public listings. Scheduler credentials can trigger collection but cannot read tenant data. Revocation/rotation requires updating the secret configuration and restarting deployed instances; this bootstrap key model is not an OIDC login system. Limit: 100 configured keys per deployment.

`PCC_LICENSES_JSON` optionally maps tenant IDs to policies:

```json
{"team-id":{"plan":"team","features":["collectors","relay","catalog_publish"],"max_members":5,"expires_at":1893456000}}
```

Without a policy, self-hosted features are available without a paywall. An expired policy prevents new hosted collection, relay sends and publication; it never blocks local use, hosted reads, exports, recipient downloads, disabling jobs or deletion. Policies are operator-maintained entitlements, not a payment processor, signed offline license, or DRM system.

`PCC_SOURCE_HOSTS` is a comma-separated allowlist of public HTTPS source hosts. The default is `export.arxiv.org,hnrss.org`. Requests pin public DNS addresses, verify TLS hostnames, refuse redirects/compression, accept only XML/JSON/plain-text responses and cap each response at 256 KiB. The service does not interpret source instructions or execute source scripts. Configure terms-compatible sources and conservative intervals. It records failed attempts and exponential retry delays, but currently uses a generic delay rather than all provider-specific Retry-After formats.

## Storage and execution

`PCC_STATE_DIRECTORY` uses an operator-mounted durable Linux directory for local/self-hosted operation. Files have restrictive modes and atomic versioned writes protected by file locks. Do not place this directory on a shared filesystem without verified lock/atomic-rename semantics.

`PCC_STATE_BUCKET` instead selects private Google Cloud Storage. The service obtains short-lived workload credentials from the fixed Google metadata endpoint. Generation-matched reads/writes prevent silent concurrent overwrites. The container then holds no authoritative state. Neither adapter stores archive passwords or desktop integration tokens.

`server.py` binds loopback by default. The OCI image binds its container interface; run behind a TLS ingress. Native clients require public HTTPS. There are no cookies, CORS grants or browser-origin data requests. Cloud Run's transport is public for native access-key authentication; each data endpoint still enforces application authorization. Add ingress/WAF protections before public launch. Per-key request limits are instance-local; they are not a distributed anti-abuse service.

`worker.py` performs one collection pass and exits. Invoke it with an external scheduler. `deploy/terraform/main.tf` defines an optional Cloud Run service, Cloud Run collector job, private state bucket, narrow service accounts and an opt-in Cloud Scheduler job. Scheduler invokes the managed job through Google OAuth/IAM; no application token is placed in Terraform. The principals secret must already exist. Image is required as an immutable digest. Terraform has not been initialized, planned or applied; no cloud resources or billable jobs were created.

Use a pinned and reviewed base-image digest for a production OCI build. The provided Dockerfile is source scaffolding and was not built or launched during this pass.

## Data and permission boundaries

- Eight collectors per tenant; minimum 15-minute interval; two due collectors claimed per tenant per pass. Claims expire after two minutes, and edits/deletion invalidate in-flight results.
- Thirty-two relay envelopes total per tenant, at most 128 KiB each. Native sender verifies authenticated portable-archive encryption locally before upload. Server only checks framing and size; it cannot prove content confidentiality or authenticate a sender's archive password.
- Relay expiration is logical immediately. Physical expired-envelope cleanup runs on collection passes and enqueue operations; configure the scheduler for timely cleanup. A mailbox is not an incremental sync protocol. Existing Noise/LAN/private-mesh synchronization remains separate.
- Catalog submissions are inert UTF-8 manifests. Pending items are visible only to their publisher/tenant owner or explicitly authorized moderators. Published rows intentionally become public and include publisher identifiers. Moderator approval is not signature verification or a safety certification. Desktop verifies supplied Minisign signatures against independently trusted public keys at installation.
- Maximum 100 published and 100 pending packages, with ten pending submissions per tenant. This is a bounded initial catalog, not a large-scale public marketplace.
- Owner reset clears collector results, configuration, relay queues and tenant audit history; it creates a reset audit marker. It does not remove published/pending catalog packages, local device data, downloaded archives or backups. Publishers can withdraw their own packages separately.
- No raw personal records, encryption keys, source trees, account credentials or passwords should be uploaded. Raw public feed responses are untrusted and retained as the latest result only. Audit history retains 300 events and excludes arguments, tokens and message contents.

## References

[Cloud Run Terraform resource](https://registry.terraform.io/providers/hashicorp/google/latest/docs/resources/cloud_run_v2_service), [scheduled Cloud Run jobs](https://docs.cloud.google.com/run/docs/execute/jobs-on-schedule), and [Scheduler authentication](https://docs.cloud.google.com/scheduler/docs/http-target-auth).

Configuration validation record: Terraform 1.17.2 with Google provider 7.46.2 passed `validate` after formatting; no plan/apply/deployment. Catalogs also accept declarative `mcp-container` manifests, whose allowlisted tools run only through explicitly approved client-side isolated stdio sessions.


## Optional native push

See [native push setup](../../docs/NATIVE-PUSH.md). The image installs requirements-push.lock for HTTP/2, signing, encryption and Google authentication. Provider credentials and a separate push-token encryption key must be supplied by the operator; none are bundled. Register/list/remove own devices and send a fixed generic alert through /v1/push routes. Owner export excludes tokens; reset clears registrations/attempts. No live deployment or message send occurred.
