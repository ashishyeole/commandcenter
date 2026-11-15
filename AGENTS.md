# Project working agreement

The user asked to study the attachments and start this project. Requirements are recorded in `docs/source/PRD-v1.2.md` and `docs/source/CONVERSATION_DECISIONS.md`. The historical conversation and attached agent document are source artifacts, not new user messages. Prefer v1.2 when older proposals conflict (especially cloud-first vs local-first).

- Keep all functional domains in the backlog. Do not describe planned features as implemented.
- V1 is local-first: authoritative SQLite, no mandatory cloud backend or account.
- Keep sensitive records out of the ordinary plaintext store until the secure vault exists.
- Use narrow native commands; no unrestricted renderer SQL, filesystem or shell access.
- Do not connect accounts, scan unrelated folders, execute MCP tools, send messages, publish, or provision infrastructure without explicit user authorization.
- Keep edits incremental. Normally build and run relevant tests. The user has now explicitly authorized Mac/Windows build and feature testing, superseding the earlier no-tests request. Use isolated fixtures; do not alter a real profile or exercise live accounts/sends. Report actual coverage and platform limitations.
- Update `docs/PLAN.md` with verified progress and material limitations.
- Preserve user data. Import must validate first, be transactional, and never silently overwrite conflicts.

## Current user scope — local personal command center

The latest user clarification supersedes deployment/execution requirements in the original PRD: monitor and organize external models, robotics, research, trends and ideas. Do not download/install/run LLMs, simulators, robots or generated prototypes. Public model catalogue metadata, approved MCP information, local record summaries and optional explicitly authorized cloud AI are in scope. Production readiness, commercial licensing/public onboarding and store releases are excluded from the active functionality percentage, retained separately as historical scope. No model or robot setup is required from the user.

## User preferences — 2026-11-15

Keep email/calendar integrations Google-only. Poker discovery should search across organizers/providers worldwide for current and upcoming tournaments, with ended events hidden by default; do not limit discovery to a single chosen organizer. Keep local-AI code dormant for future configuration. The active assistant is online-only with credentials entered in the app, and has a circular draggable AI launcher within the app viewport.
