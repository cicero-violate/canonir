RULES TO FOLLOW
when reading json files always use python to read the data

Shell Text Processing Rules

General Tooling Rules

rg (ripgrep)
- Use to search and locate patterns across files.
- Use to identify symbols, functions, flags, or heuristic surfaces.
- Always prefer over grep.

awk
- Use for line-oriented processing.
- Use for column/field extraction and structured slicing.
- Use for lightweight transformations without structural parsing.

apply_patch
- Use for deterministic source code modifications.
- Use for all file edits inside the repository.
- Never manually edit files outside apply_patch.

perl
- Use for brace-balanced or structure-aware extraction.
- Use for multi-line parsing.
- Use when nested blocks or function-level capture is required.

Decision Rule
- rg to find.
- awk to slice.
- apply_patch to modify.
- perl for structure-aware parsing.
