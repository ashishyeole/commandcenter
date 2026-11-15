# Personal AI Command Center — Conversation decisions and requirements handoff

This is a structured summary of the product-design discussion, NOT a verbatim export of the ChatGPT conversation. The complete original chat remains in ChatGPT.

## Evolution
- Initial concept: personal desktop intelligence hub for GitHub trends, LLMs/models, emerging tech, YouTube channels and content ideas, app-store movers, robotics (aquatic, aerial, ground, humanoid), daily news, learning, reminders, local AI, local memory, and extensibility.
- Expanded: agri-tech innovations, agricultural expos and equipment; poker tournament calendars, satellites and alerts; local business discovery (Austin initially, global later), website audits, sales leads, follow-up CRM, proposals, revenue tracking; LLC/company admin, email, banking, cards, finances and documents; MCP server discovery, rating, security, connection and orchestration; AI agents and personalized daily briefings.
- Added: instant AI voice/text note-taking, pinning, permanent/persistent tasks, recurring/forever tasks, universal search and navigation, proactive personal chief-of-staff, knowledge graph, idea lab, project builder, content studio, price watchers, opportunity engine.
- Product scope: All modules remain equally valid requirements even if staged. Full-featured Windows/macOS desktop; lightweight native iOS/Android companion. The assistant should navigate directly to the relevant in-app information or take an approved action via natural language.
- Project intelligence: When user moves from discovery to implementation, track local authorized project folders, git repositories, IDE/environment/LLM, commits, tasks, progress, last session and device, and resume context. Use local direct access or MCP/tools as appropriate, with user authorization and clear observation vs inference.
- Architecture revised: LOCAL-FIRST, no mandatory cloud backend, local SQLite and local AI, internet for discovery and third-party APIs, portable data export/import and encrypted backup in V1. Optional V2 trusted same-Wi-Fi/LAN peer-to-peer sync, with secure pairing, incremental changes, conflicts, deletions, and device history. Optional user-owned cloud backup, and optional future internet relay; minimize cloud cost. Google Cloud is not required in V1.
- Security: TLS for network communications, protection against CSRF where applicable, encryption of banking and personal sensitive documents, secrets in OS secure stores, least privilege and consent for local file and external account access. Data ownership, easy export, backup before app-controlled reset, uninstall limitations acknowledged.
- Packaging: Windows signed EXE installer, macOS signed/notarized DMG; future iOS App Store and Google Play mobile companion.

## Product design rules
- Do not silently omit any domain/module because it is not MVP.
- Fast universal command bar, keyboard shortcut and voice input are core UX.
- Unified local objects for notes, tasks, saved discoveries, projects, contacts, and reminders; cross-link across modules.
- User-approved side effects, especially outreach, financial actions, deletion and external tools.
- Prioritize a working vertical slice: desktop shell -> local DB -> command bar -> notes/tasks -> AI search -> research discovery -> project tracking -> backups, then add modules.
- Peer-to-peer sync is a V2 enhancement, not a blocker for V1.

## Source of truth
Read `personal_ai_command_center_prd_v1_2_portable_peer_sync.md` first. Earlier PRDs are included as historical references. Resolve conflicts in favor of v1.2 and the decisions in this file.
