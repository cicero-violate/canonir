| Feature                    | What it Adds                    | Event Impact             | Priority     | Exists In       | Why                                              |
| -------------------------- | ------------------------------- | ------------------------ | ------------ | --------------- | ------------------------------------------------ |
| Workspace-Aware Prompting  | Inject files + tree into prompt | ❌                       | **Critical** | Claude + Codex  | Both read codebase before acting ([Claude][1])   |
| BM25 File Search           | Query → relevant files          | ❌                       | **Critical** | Codex           | Built-in semantic file search layer              |
| Completion / Max Iteration | Loop termination                | `LoopFinished`           | **High**     | Claude          | Stop-hook prevents infinite loops                |
| Rich Error Feed            | Structured compiler errors      | extend `LoopObserved`    | **High**     | Both            | Compiler feedback drives fixes ([HackerNoon][2]) |
| Severity Scoring           | Weighted verification           | extend `LoopVerified`    | **High**     | Claude          | Confidence filtering system                      |
| Repeated-Failure Guard     | Prevent retry loops             | ❌                       | **High**     | Both            | Avoids redundant retries                         |
| Pre-Execution Safety       | Block destructive commands      | extend `LoopActed`       | **High**     | Claude + Codex  | Hooks / exec policy systems                      |
| Replanning Context         | Force divergence                | ❌                       | **Medium**   | Claude          | Planner avoids repeating same approach           |
| Phase Tracking             | Structured workflow             | extend `LoopObserved`    | **Medium**   | Claude          | Multi-phase gated workflow                       |
| Crash Recovery Context     | Resume intelligently            | extend cursor            | **Medium**   | Codex + Claude  | Persistent state / session recovery              |
| Check → Signal Integration | Logs → structured signals       | `CheckResult` (optional) | **High**     | Both (implicit) | Turns errors into decisions                      |
| Parallel Planning          | Multi-perspective plans         | ❌                       | **Low**      | Claude          | Multiple agents run in parallel                  |
| Sub-agents                 | Independent workers             | new events (optional)    | **Low**      | Both            | Codex threads, Claude agents                     |
| Tool Gate / Approval       | Controlled execution            | ❌                       | **Medium**   | Codex           | Blocks mutating operations until approved        |
| Context Compaction         | Prevent prompt overflow         | ❌                       | **Medium**   | Both            | Manage long context windows                      |

[1]: https://code.claude.com/docs/en/overview?utm_source=chatgpt.com "Claude Code overview - Claude Code Docs"
[2]: https://hackernoon.com/coding-rust-with-claude-code-and-codex?utm_source=chatgpt.com "Coding Rust With Claude Code and Codex"
