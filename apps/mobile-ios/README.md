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
