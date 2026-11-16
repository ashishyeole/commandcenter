## Current v0.5 local companion status

Encrypted vault, optional peer sync, local reminders, native-push registration and mobile plan approvals are now implemented in source. The historical milestone notes below describe earlier builds. Latest addition: select files up to 10 MiB into encrypted private records, replace/remove attachments in reviewed drafts, and preview bounded text/PNG/JPEG content entirely in memory. Text previews stop at 12,000 bytes; images are bounded to 20 megapixels and 1600-pixel previews. Other file types retain originals for encrypted backup. No temporary plaintext files or automatic uploads are created.

The iOS Simulator app and Android ARM64 debug APK compile. Mobile file-provider/lifecycle gestures, push delivery and real-device acceptance remain unverified. Backgrounding locks the vault; Android stores only the selected URI until you unlock and attach it to an editable record. No LLM or hardware is installed. Store signing/release is outside the current scope.

# iOS companion

Open `CommandCenter.xcodeproj` in Xcode. The target is a native SwiftUI application with an iOS 17 deployment target. Choose your own signing team before installing on a physical device; no team or provisioning profile is embedded.

```sh
xcodebuild -project apps/mobile-ios/CommandCenter.xcodeproj \
  -scheme CommandCenter -destination 'generic/platform=iOS Simulator' \
  -configuration Debug CODE_SIGNING_ALLOWED=NO \
  -derivedDataPath target/mobile-ios build
```

Implemented source: independent SQLite capture, notes/tasks/saved links, local text search, pins, daily/weekly/monthly/yearly recurrence, on-device-only dictation with permission prompts, JSON document export and transactional missing-only import. Data resides in the app sandbox and has device file protection; ordinary notes are not a dedicated encrypted vault.

Desktop → Data & privacy → Mobile companion exchange creates the same JSON envelope. Existing IDs are never overwritten on import. This does not merge edits or synchronize devices.

Not included: native discovery feeds, local LLM inference, sync/pairing, mobile vault, notifications, approval routing, complete privacy/store assets, signing or App Store release. Speech availability depends on device/language. No runtime tests were executed in this pass.

## v0.3 continuation

The unsigned Simulator app compiles with live opt-in GitHub/Hacker News discovery, bounded HTTPS reads without redirects, SQLite feed cache, save-to-library and desktop-compatible custom recurrence strings. No Simulator launch or runtime tests were performed. Project sync, sensitive vault and remote approvals remain open.
