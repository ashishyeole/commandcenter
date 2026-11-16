# Desktop appearance

Choose a theme in the header or **Settings → Appearance**. Changes save immediately on this device and survive restart. Existing profiles default to Monochrome. Portable archive configuration can include the theme; permissions remain device-local.

| Theme | Palette | Typeface |
| --- | --- | --- |
| Monochrome | Silver, black and gray | Geist Mono |
| Lumen | Warm paper and muted teal | Geist Sans |
| Slate | Cool gray and blue | Geist Sans |
| Midnight | Charcoal, silver and violet | Geist Mono |

The variable fonts are bundled locally under the SIL Open Font License. Their license and upstream revision/hashes are in `apps/desktop/public/fonts/`. No remote font request is required.

The sidebar groups everyday tools, exploration and management. Settings separates appearance, reminders, connections and system updates. Discover separates browsing, research radar and saved research. Data & privacy separates portable archives, large/scheduled backups, cleanup and mobile exchange. Workspace domains use a grouped selector with counts. Section tabs support arrow keys, Home and End, and keep panels mounted so switching sections preserves unsaved forms. Source descriptions, verification notes and timestamps remain available under **Details & provenance** or **Source scope & limitations**.

The draggable circular assistant launcher contains only its Sparkles image. Its accessible name and tooltip remain available. Opening the panel retains the online-only reviewed-request flow; local-AI source stays dormant.

YouTube cards display full-color public video thumbnails; GitHub cards can display owner avatars. Images use fixed `i.ytimg.com` and `avatars.githubusercontent.com` CDN paths derived from validated public source URLs, with no referrer, account token or forwarded query string. Other arbitrary image hosts are not enabled. Failed images hide without removing card information. Images are not converted to grayscale by any theme.

Selected native Mac checks are recorded in [themes-native-qa.json](testing/themes-native-qa.json). They cover theme rendering/persistence, the launcher, color thumbnails, section navigation, preserved draft fields and grouped selectors. Full assistive-technology, performance and every-feature acceptance remain open.
