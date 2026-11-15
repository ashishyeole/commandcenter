# Dormant local AI reference

Preserved at the user’s request for later local-AI configuration. These files are outside the frontend build include paths and the Rust files in ../src-tauri/dormant are not registered modules or IPC commands. They never run, install or download a model in the current app.

Reference snapshot: commit 78f4ea6. LocalAssistant.tsx, ModelRuntime.tsx and model_runtime.rs preserve the former UI/runtime management. connectors.reference.rs preserves the former local inference and embedding helpers alongside their original surrounding code. Relative imports will need review before any future reactivation; these are preserved source, not enabled integrations.

The active assistant uses CloudAssistant and requires an API key entered directly into the application’s secure-store setup. Record briefings continue to work without a model. Reactivating local inference or downloads requires a later user instruction.
