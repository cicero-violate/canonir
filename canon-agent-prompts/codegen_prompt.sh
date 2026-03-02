#!/usr/bin/env bash
# -----------------------------------------------------------
# codegen_prompt.sh
# Generates gpt_5_2_prompt_apply_patch.md
# Examples are read live from the .patch files at runtime.
# -----------------------------------------------------------

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OUTPUT="$SCRIPT_DIR/gpt_5_2_prompt_apply_patch.md"

# -----------------------------------------------------------
# Helper: reads a .patch file and indents it into a fenced
# code block. Strips any trailing whitespace.
# -----------------------------------------------------------
inject_patch() {
    local file="$1"
    echo '```'
    sed 's/[[:space:]]*$//' "$file"
    echo '```'
}

# -----------------------------------------------------------
# Resolve patch file paths
# -----------------------------------------------------------
P1="$SCRIPT_DIR/example1_single_function.patch"
P2="$SCRIPT_DIR/example2_two_functions.patch"
P3="$SCRIPT_DIR/example3_new_file.patch"
P4="$SCRIPT_DIR/example4_delete_file.patch"
P5="$SCRIPT_DIR/example5_multiple_files.patch"

# -----------------------------------------------------------
# Validate all patch files exist before we start writing
# -----------------------------------------------------------
for f in "$P1" "$P2" "$P3" "$P4" "$P5"; do
    if [[ ! -f "$f" ]]; then
        echo "ERROR: missing patch file: $f" >&2
        exit 1
    fi
done

# -----------------------------------------------------------
# Generate the markdown
# -----------------------------------------------------------
# -----------------------------------------------------------
# Section 1: Header + format envelope
# -----------------------------------------------------------
cat > "$OUTPUT" <<'SECTION1'
## apply_patch

To edit files, you must use the `apply_patch` tool by including patches in **code blocks**. The proxy will automatically extract and apply them.

**Format - Your patch language is a stripped‑down, file‑oriented diff format:**

```
*** Begin Patch
[ one or more file sections ]
*** End Patch
```

Within that envelope, you get a sequence of file operations.
You MUST include a header to specify the action you are taking.
Each operation starts with one of three headers:

*** Add File: <path> - create a new file. Every following line is a + line (the initial contents).
*** Delete File: <path> - remove an existing file. Nothing follows.
*** Update File: <path> - patch an existing file in place (optionally with a rename).

**Note:** The examples below are the source of truth — if the rules and examples conflict, follow the examples.

SECTION1

# -----------------------------------------------------------
# Section 2: Add File example
# -----------------------------------------------------------
echo "**Adding a new file** — every line prefixed with \`+\`:" >> "$OUTPUT"
inject_patch "$P3" >> "$OUTPUT"
echo "" >> "$OUTPUT"

# -----------------------------------------------------------
# Section 3: Delete File example
# -----------------------------------------------------------
echo "**Deleting a file** — nothing follows the header:" >> "$OUTPUT"
inject_patch "$P4" >> "$OUTPUT"
echo "" >> "$OUTPUT"

# -----------------------------------------------------------
# Section 4: Update File + Context Markers explanation + examples
# -----------------------------------------------------------
cat >> "$OUTPUT" <<'SECTION4'
**Updating a file** — use `@@` to mark where changes go:
- `@@` is a separator — it marks the boundary between two unrelated hunks in the same file
- Place `@@` on its own line before the `-` lines of each hunk
- Do NOT put anything after `@@` on the same line — no function signatures, no comments
- Do NOT put a closing `@@` at the end — let `*** End Patch` or the next file header terminate the hunk
- If you have only ONE hunk, you still need a single `@@` before the `-` lines
- If you have TWO unrelated hunks in the same file, put a solo `@@` between them
- `-` lines are removed, `+` lines are added, unprefixed lines are context to anchor the hunk
- **NEVER put a trailing `@@` before *** End Patch or before the next file header**
- File references can only be relative, NEVER ABSOLUTE
- Do NOT use unified diff line numbers (e.g., `@@ -3,6 +3,7 @@`) - the tool infers positions automatically
- **PREFER `*** Delete File` + `*** Add File` over `*** Update File` when replacing most or all of a large file — removing every line individually is highly inefficient**

Single hunk — one `@@`, multiple edits in the same block:

SECTION4
inject_patch "$P1" >> "$OUTPUT"
echo "" >> "$OUTPUT"

echo "Two unrelated hunks in the same file — solo \`@@\` between them:" >> "$OUTPUT"
inject_patch "$P2" >> "$OUTPUT"
echo "" >> "$OUTPUT"

# -----------------------------------------------------------
# Section 5: Multiple files in one patch
# -----------------------------------------------------------
echo "**Multiple files in one patch** — Update and Add in the same envelope:" >> "$OUTPUT"
inject_patch "$P5" >> "$OUTPUT"
echo "" >> "$OUTPUT"

echo "Generated: $OUTPUT"
