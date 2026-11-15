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
