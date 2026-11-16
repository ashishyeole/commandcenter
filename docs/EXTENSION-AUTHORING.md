# Local extension authoring

The SDK is declarative JSON. It contributes an approved information request, not renderer code. The app never runs JavaScript from a manifest. Existing built-in remote MCP connections remain the preferred path for remote MCP servers.

Create a new inert REST manifest:

```
node scripts/create-extension.mjs my-public-feed "My public feed" https://example.com/feed.json /tmp/my-public-feed.json
```

The helper refuses to overwrite files. Open the result in Extensions, paste the exact JSON, and use **Validate manifest**. Validation does not install or contact the destination. Export preserves the exact authored JSON. Installation requires typing its name; unsigned local packages require explicit unverified-publisher approval. Installed packages start disabled. Enabling, request preview and each execution have separate controls.

Schema 1 fields are `schema`, `id`, `name`, `version`, `description`, `publisher`, `license`, `kind`, `endpoint`, `method`, `image`, `command`, `tools`. Unknown fields are rejected. REST permits a fixed public HTTPS endpoint and GET/POST, no credentials/query/fragment in the URL, an empty image/command/tools. Credentials use the connection's device-local OS secure store. Native validation resolves public destinations before requests; redirects are refused. Request input is data, never a shell command.

Container and MCP-container manifests require an immutable `image@sha256:<64 hex digits>`, an explicit command argument array, empty endpoint/method, and, for MCP, up to 50 exact tool names. Nothing is downloaded or executed by authoring or installation. Container use requires an independently installed Docker runtime and separate consent. Local model, IDE/plugin and non-container software installation are outside the current monitoring scope.

For signed sharing, sign the **exact UTF-8 file bytes** with your own Minisign key outside the app; provide the public key and detached signature during import. Changing whitespace after signing invalidates it. Verify the publisher key through an independent trusted channel before pinning it. A valid signature establishes byte integrity and key continuity, not publisher honesty or safety. Updates show exact proposed fields and stay inert until reviewed. Test packages with synthetic data; never embed secrets in the manifest.

The executable native validator in `apps/desktop/src-tauri/src/extensions.rs` is authoritative. Limits include 16 KB manifest JSON, 30 installed extensions, 20 command arguments, and 2,000 bytes per argument. Production marketplace moderation and independent publisher identity checks are separate external work.
