# Native iOS / Android push

Native push is implemented as an optional host adapter plus native companion registration/receipt paths. It is **not activated**: no account was connected, provider key created, host deployed or notification sent during implementation. Local reminders and LAN sync work without it.

## What is sent

Only a fixed generic task-review alert and a random event identifier. Task titles, notes, financial details, files and peer keys never enter the push payload. The mobile OS/provider receives its own device token; your selected HTTPS host stores that token encrypted with a separate operator-managed AES-GCM key. Provider acceptance is recorded separately from confirmed delivery. There are no delivery-webhook claims.

## Host setup

Use the existing optional host behind HTTPS, with a personal owner/member access token configured through `PCC_PRINCIPALS_JSON`. The native phone and desktop sender must use credentials belonging to the **same tenant and subject**. Do not enter Apple or Firebase server keys on the phone or in chat. Supply server secrets through your own secret manager:

| Setting | Purpose |
| --- | --- |
| `PCC_PUSH_STORAGE_KEY_BASE64` | Base64 encoding of a cryptographically random 32-byte key, used only to encrypt registered provider tokens. Keep it separate from hosted state; loss requires re-registration. Changing it requires a migration or clearing registrations and registering again. |
| `PCC_APNS_KEY_FILE` | Mounted Apple `.p8` signing-key file, readable only by the service identity. |
| `PCC_APNS_KEY_ID`, `PCC_APNS_TEAM_ID` | Identifiers belonging to that Apple key/team. |
| `PCC_APNS_TOPIC` | Your signed iOS application's bundle identifier. |
| `PCC_FCM_PROJECT` | Your Firebase project ID. |
| Google application default credentials | A workload identity or `GOOGLE_APPLICATION_CREDENTIALS` service-account file authorized for Firebase Cloud Messaging. Enable the FCM API in your project. |

Only the platform being used needs its provider credentials. The Dockerfile installs the resolved `requirements-push.lock`; a local virtual environment can install it instead. No deployment or IAM changes were applied. Request paths are `/v1/push/devices` (register/list), `/v1/push/devices/{id}` (delete), `/v1/push/send` (explicit send). Arbitrary message bodies, external destinations and provider URLs are not accepted. Existing static-principal authentication is sufficient for personal use; no commercial onboarding is required.

Registration is restricted to own devices, bounded to 32 per tenant. Sending reserves an event before contacting the provider, deduplicates that event for 24 hours, and limits each subject to one attempt per minute and 20 per day. Ambiguous attempts are not retried automatically. Invalidated provider tokens are disabled only if the registration generation still matches. Hosted owner export omits encrypted tokens; reset clears push registrations and attempt records too. In-flight provider requests may already be delivered after revocation/reset.

## iPhone / iPad

Build/sign with your Apple team and Push Notifications capability. The project has `aps-environment`: development for Debug, production for Release. The Simulator compilation performed here does not establish APNs device delivery.

Open **Native push notifications**, enter the HTTPS host origin and your host member access token, choose the environment matching the signed build, and approve registration. Apple permission is requested before registration. APNs tokens refresh on app launch and are uploaded only while the saved consent remains enabled. Credentials stay in this device's Keychain. Copy the displayed registration device ID to the desktop settings. Failed uploads explicitly require reopening/re-registration. Foreground presentation is supported; background alerts follow iOS permissions and Focus settings. The app cannot intercept every background APNs alert to implement its own quiet hours.

## Android

The app includes Firebase Messaging 26.0.2, with SDK automatic initialization and Analytics collection disabled by default; no Analytics SDK is included. A device with Google Play services is required.

Open **Native push notifications** and supply your Firebase client configuration (SDK app ID, client API key, project ID and sender/project number), HTTPS host origin and host member token. These client values are available from your Firebase app configuration; they are not the service-account private key. Approve registration and notification permission. Saved configuration uses Android Keystore protection. Token rotation re-registers while enabled; failures are visible and retry on reopening/registration. Changing Firebase projects requires disabling/removing registration and restarting the app.

The receiver handles data-only, high-priority generic alerts, deduplicates event IDs, suppresses alerts while disabled or permission-denied, and drops alerts during 10 PM–8 AM device-local quiet hours. Delivery remains subject to Play services, connectivity, force-stop and OS power management. Tapping an alert opens the app; it never executes an agent or syncs files automatically.

## Desktop sending

In **Personal email, SMS, voice & push reminders**, choose **Native iOS / Android push**, enter your host origin, host member token and the phone's displayed device ID. Review the exact generic message and consent. Save alone does not send. You can separately approve an immediate send, or explicitly authorize a daily due-task reminder; the latter runs only while this desktop is running and respects its saved quiet hours and task snoozes. Always-on push based on private task state while every personal device is off is not implemented, because the optional host does not receive those tasks.

Disable the desktop channel to stop future sends and remove its credentials. On the phone choose **Remove registration & disable** to request remote deletion and turn off local registration. If remote removal fails, the UI says so and preserves the configuration for a retry. Already submitted notifications may still arrive; uninstalling cannot guarantee server-side cleanup. Provider errors do not expose credentials in audit history.

## Validation

Rust/TypeScript compile, Android debug build, iOS Simulator build, Python syntax compilation and dependency compatibility checks completed. Ten isolated host authorization/token-lifecycle tests with mocked provider calls now pass; see TEST-REPORT.html. No native runtime or live push delivery verified. Real delivery remains an owner setup and acceptance step.

Implementation references: [Apple registration](https://developer.apple.com/documentation/usernotifications/registering-your-app-with-apns), [Apple token authentication](https://developer.apple.com/documentation/usernotifications/establishing-a-token-based-connection-to-apns), [Firebase Android registration](https://firebase.google.com/docs/cloud-messaging/android/get-started), [Firebase receive behavior](https://firebase.google.com/docs/cloud-messaging/android/receive-messages), [FCM HTTP v1](https://firebase.google.com/docs/cloud-messaging/send/v1-api).
