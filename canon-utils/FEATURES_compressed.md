| Feature                    | What it Adds                    | Event Impact             | Priority     | Exists In       | Why                                              | Status       |
| -------------------------- | ------------------------------- | ------------------------ | ------------ | --------------- | ------------------------------------------------ | ------------ |
| Workspace-Aware Prompting  | Inject files + tree into prompt | ✅ (file tree in prompt) | **Critical** | Claude + Codex  | Both read codebase before acting ([Claude][1])   | ✅ Complete  |
| BM25 File Search           | Query → relevant files          | ❌                       | **Critical** | Codex           | Built-in semantic file search layer              | ⏳ Pending   |
| Completion / Max Iteration | Loop termination                | ✅ (`halt` gates stages) | **High**     | Claude          | Stop-hook prevents infinite loops                | ✅ Complete  |
| ToolBatchSettled Event     | Reactive tool result release    | ✅ (new event)           | **High**     | Canon           | Router reacts after all results land             | ✅ Complete  |
| Rich Error Feed            | Structured compiler errors      | extend `LoopObserved`    | **High**     | Both            | Compiler feedback drives fixes ([HackerNoon][2]) | ⏳ Pending   |
| Severity Scoring           | Weighted verification           | extend `LoopVerified`    | **High**     | Claude          | Confidence filtering system                      | ⏳ Pending   |
| Repeated-Failure Guard     | Prevent retry loops             | ❌                       | **High**     | Both            | Avoids redundant retries                         | ⏳ Pending   |
| Pre-Execution Safety       | Block destructive commands      | extend `LoopActed`       | **High**     | Claude + Codex  | Hooks / exec policy systems                      | ⏳ Pending   |
| Replanning Context         | Force divergence                | ❌                       | **Medium**   | Claude          | Planner avoids repeating same approach           | ⏳ Pending   |
| Phase Tracking             | Structured workflow             | extend `LoopObserved`    | **Medium**   | Claude          | Multi-phase gated workflow                       | ⏳ Pending   |
| Crash Recovery Context     | Resume intelligently            | extend cursor            | **Medium**   | Codex + Claude  | Persistent state / session recovery              | ⏳ Pending   |
| Check → Signal Integration | Logs → structured signals       | `CheckResult` (optional) | **High**     | Both (implicit) | Turns errors into decisions                      | ⏳ Pending   |
| Parallel Planning          | Multi-perspective plans         | ❌                       | **Low**      | Claude          | Multiple agents run in parallel                  | ⏳ Pending   |
| Sub-agents                 | Independent workers             | new events (optional)    | **Low**      | Both            | Codex threads, Claude agents                     | ⏳ Pending   |
| Tool Gate / Approval       | Controlled execution            | ❌                       | **Medium**   | Codex           | Blocks mutating operations until approved        | ⏳ Pending   |
| Context Compaction         | Prevent prompt overflow         | ❌                       | **Medium**   | Both            | Manage long context windows                      | ⏳ Pending   |

[1]: https://code.claude.com/docs/en/overview?utm_source=chatgpt.com "Claude Code overview - Claude Code Docs"
[2]: https://hackernoon.com/coding-rust-with-claude-code-and-codex?utm_source=chatgpt.com "Coding Rust With Claude Code and Codex"
