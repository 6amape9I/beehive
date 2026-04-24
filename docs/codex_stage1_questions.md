# Stage 1 Questions And Resolutions

## 2026-04-23

- Question: How should communication with the instruction creator and separate markdown answers be represented?
- Resolution: Use in-repository markdown logs under `docs/`.
- Files: `docs/codex_stage1_questions.md`, `docs/codex_stage1_progress.md`, `docs/codex_stage1_instruction_checklist.md`, `docs/codex_stage1_delivery_report.md`.

No unresolved product questions are currently blocking implementation.

## 2026-04-24

- Question: How should Stage 1 handle workdir paths entered manually when relative paths resolve inside the app tree and trigger `tauri dev` rebuilds?
- Options considered:
  - accept relative paths and keep the current behavior;
  - normalize relative paths against the runtime directory;
  - reject relative paths and require a workdir outside the application directory.
- Recommended answer: Reject relative paths and require the workdir to live outside the application directory.
- Resolution chosen: Implemented the recommended answer. The backend now rejects relative paths and any workdir path nested under the current application directory, and the UI explains this requirement next to the path input.
