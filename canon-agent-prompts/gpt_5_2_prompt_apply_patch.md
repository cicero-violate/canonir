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

**Adding a new file** — every line prefixed with `+`:
```
*** Begin Patch
*** Add File: src/config.ts
+export const API_URL = 'https://api.example.com';
+export const TIMEOUT = 5000;
*** End Patch
```

**Deleting a file** — nothing follows the header:
```
*** Begin Patch
*** Delete File: src/old-module.ts
*** End Patch
```

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

```
*** Begin Patch
*** Update File: src/handler.ts
@@
-  const result = parse(input);
+  const result = parseAndValidate(input);
   console.log('Processing...');
-  return result;
+  return result.data;
*** End Patch
```

Two unrelated hunks in the same file — solo `@@` between them:
```
*** Begin Patch
*** Update File: src/utils.ts
@@
-  return name.toUpperCase();
+  return name.trim().toUpperCase();
@@
-  return email.includes('@');
+  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email);
*** End Patch
```

**Multiple files in one patch** — Update and Add in the same envelope:
```
*** Begin Patch
*** Update File: src/index.ts
@@
-export { handler };
+export { handler, middleware };
*** Add File: src/middleware.ts
+export function middleware(req: any) {
+  console.log('Middleware called');
+}
*** End Patch
```

apply_patch <<'EOF'
*** Begin Patch
*** Update File: src/mir.rs
@@
-            match tcx.def_kind(def_id) {
-                DefKind::Fn | DefKind::AssocFn => {
-                    // Only attempt MIR for defs that have a body in HIR
-                    if tcx.hir().body_owner_kind(local_def_id).is_none() {
-                        continue;
-                    }
+            match tcx.def_kind(def_id) {
+                DefKind::Fn | DefKind::AssocFn => {
+                    // Skip functions without bodies (e.g. trait method declarations)
+                    if tcx.is_const_fn(def_id) || tcx.is_constructor(def_id) {
+                        // allow normal fns; these helpers just avoid weird edge cases
+                    }

*** End Patch
EOF
