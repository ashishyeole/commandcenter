# Acceptance checks

Current verification is recorded in [TEST-REPORT.html](TEST-REPORT.html), [testing/results.json](testing/results.json) and [Discovery aggregation](DISCOVERY-AGGREGATION.md). The dated implementation records below are historical; earlier locked-Mac, unbuilt-Windows and skipped-test statements do not describe the latest checks. Full PRD acceptance remains open.

## Automated

- `npm run build`: strict TypeScript and production frontend build.
- `cargo test -p command-center-core`: file-backed reopen/persistence, stable device identity, FTS search, recurrence month boundary, optimistic conflict rejection, journal count, import idempotence and conflict preservation, invalid-import atomicity, tombstones, hostile search syntax, portable round trip, wrong password, authenticated tamper rejection, CSV formula handling.
- `cargo build -p personal-command-center`: native bridge and desktop shell compile.

## Native manual workflow

Use non-sensitive test records. Run `npm run desktop`.

1. Create and pin a note; create monthly task due Jan 31 and a saved HTTPS URL.
2. Search by title/body using Cmd/Ctrl+K; use keyboard Tab/Enter and Escape on dialogs.
3. Complete the recurring task; verify Feb 28 next date and active state. Disable repeat when no longer desired.
4. Quit and reopen; verify all data and pins persist, with internet disconnected.
5. Export a ZIP to a new path and inspect JSON/CSV/Markdown outside the app.
6. Export `.pacc` with a test password. Wrong password must report failure. Correct password verifies and previews.
7. Restore the same archive: all unchanged records are duplicates. Edit a record then re-preview: conflict is reported and local content kept.
8. Clean-profile restore: all original IDs and current record contents preserved. Never remove a real profile to perform this test; use a separate OS test user.
9. Verify cancelling file dialogs leaves data untouched. Settings state is truthful about plaintext local storage, missing backup history and unavailable reset.

Windows runtime, signing/notarization, screen-reader audit, notification scheduling, mobile and actual LAN sync are not covered by this foundation build.

## Setup verification — 2026-11-14

Passed locally on macOS:
- TypeScript check and Vite production build.
- Seven Rust core tests, including checksum corruption and future-schema rejection.
- Native Rust application compilation.
- Tauri debug macOS `.app` bundling with embedded frontend assets.
- Rust formatting check.

Native visual inspection was attempted but blocked because the Mac was locked and the computer-use tool could not unlock it. No native UI workflow, screenshot or end-to-end dialog test is claimed. The tests cover repository and archive behavior directly. Windows CI configuration exists but has not run. DMG/EXE packaging targets are configured, but no signed installer was generated.

Local build artifact: `target/debug/bundle/macos/Personal Command Center.app`. Launch it after unlocking, or use `npm run desktop` for development.

## v0.2 implementation pass — 2026-11-14

The user explicitly requested no tests. No Rust test suite, UI test, live connector check, model request, vault unlock, document ingestion, reset or real-account integration was executed for v0.2. Historical v0.1 test results do not establish correctness of the expanded implementation.

Compilation completed for the frontend and native desktop bridge during development. A macOS `.app` bundle was produced. The default DMG helper failed on a Finder AppleEvent timeout, so a plain disk-image packaging path is used instead. Final packaging and iOS compilation results are recorded below when available. Android source is present but Gradle is unavailable, so no Android build is claimed.

New runtime acceptance work still needed: schema upgrades against actual user data, source response/error behavior, local model filtering/context/cancellation, semantic index refresh, folder authorization/revocation, native notifications/shortcut conflicts, vault expiry/key-loss/recovery, selective and private archive restoration, copy-conflict graph preservation, native mobile dictation/import validation, and cross-platform packaging. None of this was inferred from a compile-only check.

Final compile record for v0.2:
- `npm run build`: TypeScript and frontend production bundle completed.
- Tauri `build --debug --bundles app`: native macOS compilation and `.app` bundle completed with the final desktop source.
- iOS `xcodebuild` for generic iOS Simulator with code signing disabled: completed with exit 0; executable produced under `target/mobile-ios-final/Build/Products/Debug-iphonesimulator/CommandCenter.app`. Simulator destination warnings remain. No simulator was launched and speech/storage runtime behavior was not exercised.
- Android: source only, not compiled (Gradle unavailable).
- No automated test suites or UI tests run for this pass.
- Plain macOS DMG packaging completed successfully using `scripts/package-mac-plain.sh`, avoiding Finder automation. Artifact: `target/debug/bundle/dmg/Personal Command Center_0.2.0_aarch64.dmg`. It contains the debug app and Applications shortcut; it is not Developer-ID-signed/notarized and was not installation-tested.

## v0.3 compile/package record — 2026-11-14

- `npm run build`: passed (TypeScript and Vite production assets).
- `cargo check -p personal-command-center --offline`: passed, including new scheduling, backup and portability code.
- `npm run tauri -w apps/desktop -- build --debug --bundles app`: passed; v0.3 macOS development `.app` bundled.
- `./scripts/package-mac-plain.sh`: passed with macOS disk-image services outside the sandbox; `target/debug/bundle/dmg/Personal Command Center_0.3.0_aarch64.dmg` produced.
- `xcodebuild -project apps/mobile-ios/CommandCenter.xcodeproj -scheme CommandCenter -sdk iphonesimulator -configuration Debug -derivedDataPath target/mobile-ios-final CODE_SIGNING_ALLOWED=NO build`: passed with Xcode compiler services outside the sandbox. Simulator build only; no app launch or iPhone signing.
- Android `:app:assembleDebug`: passed with cached Gradle 8.14, Temurin Java 17 and the installed Android SDK, after fetching declared build dependencies and fixing a Compose `Card` argument compile error. Output: `apps/mobile-android/app/build/outputs/apk/debug/app-debug.apk`. Debug key only; no Play signing or device launch. Gradle reports deprecated features that need attention before Gradle 9.

The Android environment was found under existing machine caches rather than on PATH. No SDK/JDK installation was required. The earlier v0.2 “not compiled” record remains historical. Build logs for this pass were captured under `/tmp/pcc-{desktop,ios,android}-build.log` and may be removed by the OS.

No unit, integration, UI, security, restore, network or acceptance tests were run. All 16 PRD workflows remain **unverified in v0.3**. Compilers do not establish runtime correctness, safe recovery, network interoperability or release readiness. Windows binaries and production-signed distributions remain undelivered.

## v0.4 V2 implementation — 2026-11-14

Compile/package checks completed: workspace Rust compilation, TypeScript/Vite, macOS debug `.app`, shared Rust ARM64 iOS Simulator static library, shared Rust Android ARM64 JNI library, native iOS Simulator app, and Android debug APK. The updated plain macOS DMG uses v0.4.0 and explicit local-network permission metadata. Artifact hashes and exact paths are recorded in BUILD-ARTIFACTS.json.

No tests, app launch, peer listener activation, real pairing/sync, mobile migration execution, private transfer, WebDAV request, vault unlock, external account access, reset or production release was performed. The V2 report’s 100% is tracked source-scope coverage, including existing private mesh connectivity as the relay alternative. It is not runtime acceptance, release readiness, or completion of the wider V1/V3 backlog.

Deferred acceptance includes interrupted/reordered exchange; simultaneous edits/deletes; trust changes during active operations; mobile background/screen-exit cancellation; notification receipts; scope boundaries; key loss/replacement; encrypted private-copy retry/conflicts; WebDAV TLS/auth/ETag failures; backup recovery; migration of real mobile databases; and cross-platform installation. Windows and physical iOS device signing remain unverified/unconfigured.

Final package inspection caught and corrected Xcode resolving a development Rust dylib through the Cargo output search path. The project now searches only the staged static-library directory. The rebuilt iOS bundle defines `pcc_mobile_request`/`pcc_mobile_free` internally and has no development `libpcc_mobile_peer.dylib` dependency. Android APK inspection confirms the ARM64 JNI library is included and depends only on Android system libraries. These are artifact/linkage checks, not app/runtime tests.

## v0.5 compile/package record — 2026-11-14 local date

- Workspace Rust checks and TypeScript/Vite production builds passed for the expanded source, including recurring research plans, sandboxed stdio MCP, document extraction, provider integrations and optional hosted-service client.
- macOS debug `.app` bundled successfully. The Swift speech library is inside the app and linked via `@rpath`; the linker reports a benign duplicate Frameworks rpath. This is not Developer ID signing/notarization or installation acceptance.
- Both updated ARM64 Rust mobile cores compiled. Native iOS Simulator build succeeded with code signing disabled. `otool`/`nm` inspection confirms the mobile Rust C symbols are defined in the bundle and no development Rust dylib is required.
- Android `:app:assembleDebug` succeeded with the current shared core. Gradle deprecation warnings remain; this is an ARM64 debug-key artifact, not a Play release.
- Python syntax compilation passed for the optional host and release-manifest script; no service was started.
- Terraform 1.17.2 formatting and provider-schema validation succeeded using Google provider 7.46.2, with backend initialization disabled. No plan/apply/cloud deployment occurred.
- Dependency advisory findings and remaining warnings are recorded in `security/DEPENDENCY-ADVISORIES.json`; this is not an independent security assessment.

No unit, integration, UI, provider, peer-sync, microphone, scanner/OCR, model, tool-container, installer, migration or runtime acceptance tests were run. No accounts were connected, notifications sent, resources provisioned or releases published. All 16 PRD acceptance workflows remain unverified for current source. Production signing, Windows build/runtime, real-device behavior, accessibility/performance measurements and independent security review remain open. See BUILD-ARTIFACTS.json for the final recorded development artifact hashes.


## Current v0.5 testing pass — user authorization supersedes no-tests request

See [test report](TEST-REPORT.html) and checked-in output under testing/. Final results: 20 Rust executions (18 distinct cases; shared transport cases run in two targets), 10 Python push tests with provider calls mocked, frontend/native compilation, Mac disk-image integrity/mount/copy and deep signature verification pass. Loopback transport tests initially could not bind in the sandbox, then passed with explicit localhost execution permission. All fixtures are isolated.

Found and repaired the debug app's incomplete signature resource seal. Packaging now ad-hoc signs and verifies before creating its DMG. Gatekeeper still rejects this development distribution; production signing/notarization are outstanding. No production profile or installed app was touched. Selected Mac native UI workflows now pass in an isolated QA profile after the user unlocked the Mac. See the test report for exact coverage. Windows compilation, EXE installation and runtime testing require an accessible Windows runner and were not run; workflow configuration is not execution evidence. No full PRD acceptance workflow is newly claimed.

Native follow-up found and fixed an updater-configuration startup crash. Selected QA workflows now pass: note/search/pin, attachment import/preview, month-end recurrence, quit/relaunch persistence, plain/encrypted backup round trips, duplicate preservation and wrong-password rejection. Earlier locked-screen statements above describe historical attempts. Clean-profile reconstruction, all domain workflows, live services and Windows runtime are not established.
