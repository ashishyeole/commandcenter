## Current v0.5 local companion status

Encrypted vault, optional peer sync, local reminders, native-push registration and mobile plan approvals are now implemented in source. The historical milestone notes below describe earlier builds. Latest addition: select files up to 10 MiB into encrypted private records, replace/remove attachments in reviewed drafts, and preview bounded text/PNG/JPEG content entirely in memory. Text previews stop at 12,000 bytes; images are bounded to 20 megapixels and 1600-pixel previews. Other file types retain originals for encrypted backup. No temporary plaintext files or automatic uploads are created.

The iOS Simulator app and Android ARM64 debug APK compile. Mobile file-provider/lifecycle gestures, push delivery and real-device acceptance remain unverified. Backgrounding locks the vault; Android stores only the selected URI until you unlock and attach it to an editable record. No LLM or hardware is installed. Store signing/release is outside the current scope.

# Android companion

Open this directory in Android Studio. Native Kotlin / Jetpack Compose, Android API 31+, compile/target SDK 35, JDK 17, Android Gradle Plugin 8.9.1, Kotlin 2.1.20.

Use Gradle 8.11.1 with your installed Android SDK (set the local `sdk.dir` through Android Studio). A Gradle wrapper binary is not vendored yet:

```sh
cd apps/mobile-android
gradle wrapper --gradle-version 8.11.1
./gradlew assembleDebug
```

Implemented source: SQLite notes/tasks/saved links, local search/pins, recurring completion, microphone permission and on-device recognition where available, Storage Access Framework JSON export/import. Missing-only import is transactional. The app requests no internet permission and disables automatic OS backups; export before uninstalling.

The desktop can exchange the same ordinary-item JSON envelope. It is plaintext, excludes vault content and is not device synchronization.

Not included: live discovery, account access, sync, mobile vault, notifications, store assets/signing or release distribution. Gradle was unavailable in the implementation environment, so Android compilation and runtime behavior remain unverified. No runtime tests were executed in this pass.

## v0.3 build and discovery

A debug APK was compiled at `app/build/outputs/apk/debug/app-debug.apk`. This is a development artifact, not Play distribution. The source now includes explicitly approved GitHub/Hacker News discovery, SQLite caches, save-to-library, and desktop-compatible custom recurrence. No runtime tests or device launch were performed.

Build with Java 17, Android SDK 35 and Gradle 8.14: `gradle --no-daemon :app:assembleDebug`. The successful build used existing machine installations with JAVA_HOME and ANDROID_HOME set explicitly. Internet access is only used by the running app when the user approves a feed refresh.
