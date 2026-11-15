# Instructions for Codex

Build the Personal AI Command Center from the included PRD. First read `personal_ai_command_center_prd_v1_2_portable_peer_sync.md` and `CONVERSATION_DECISIONS.md` fully. Treat all listed modules as in scope, even when implementation is phased. Do not provision a cloud backend for V1.

Start by inspecting the current repository, if any. Propose an implementation plan and create the first vertical slice: cross-platform Tauri desktop app, local SQLite with migrations, universal command palette, notes and tasks with pinning and recurrence, searchable saved items, export/import and encrypted backup. Include tests, security boundaries, and build instructions. Prefer incremental working code over placeholder-only scaffolding. Ask for approval before irreversible actions or external account access. Record decisions and progress in the repo.

Target Windows EXE and macOS DMG packaging, with native iOS/Android companion later. Keep sync abstractions ready for V2 encrypted LAN peer-to-peer sync. Do not claim unimplemented modules are complete.
