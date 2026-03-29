# Implementation Plan: harness_suite compile-error recovery

## Problem

`harness_suite.rs` calls `first_failing_case()` which only recognises test-failure
output (`test ... FAILED`, `failures:` section, `Running unittests ...`).  When
`cargo test --workspace` fails because a crate doesn't compile, none of those
patterns match and the suite bails immediately:

```
Error: cargo test --workspace failed, but no failing crate/test could be parsed
```

This prevents the harness from ever repairing a crate whose tests won't compile.

---

## Fix — `canon-utils/canon-runtime/src/bin/harness_suite.rs`

### 1. Add `crate_from_compile_error`

```rust
/// Parses the crate name from a cargo compile-error line such as:
///   error: could not compile `canon-llm-runtime` (lib test) due to …
///   error: could not compile `canon-llm-runtime` due to …
fn crate_from_compile_error(text: &str) -> Option<String> {
    let marker = "could not compile `";
    for line in text.lines() {
        let Some(idx) = line.find(marker) else { continue };
        let rest = &line[idx + marker.len()..];
        let end = rest.find('`')?;
        let name = rest[..end].trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}
```

### 2. Add `compile_error_first_line`

```rust
/// Returns the first `error[…]` or `error:` line from cargo output, for use
/// as a synthetic "test name" when routing compile failures to the repair agent.
fn compile_error_first_line(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("error[") || t.starts_with("error: ") {
            return Some(t.to_string());
        }
    }
    None
}
```

### 3. Update `first_failing_case` to fall back to compile-error detection

```rust
fn first_failing_case(
    result: &CommandResult,
    default_crate: Option<&str>,
) -> Option<(String, String)> {
    // existing test-failure parsing …
    if let Some(found) = parse_failing_case_from_text(&result.output, default_crate) {
        return Some(found);
    }
    if let Some(test_name) = first_failed_test_name(&result.stdout) { … }
    if let Some(test_name) = first_failed_test_name(&result.stderr) { … }

    // NEW: fall back to compile-error detection
    let combined = &result.output;
    if let Some(crate_name) = crate_from_compile_error(combined)
        .or_else(|| default_crate.map(str::to_string))
    {
        // Use the first compiler error line as a synthetic "test" description
        // so the repair agent has context.
        let error_summary = compile_error_first_line(combined)
            .unwrap_or_else(|| "compile_error".to_string());
        return Some((crate_name, error_summary));
    }

    None
}
```

### 4. Add `#[cfg(test)]` coverage for the new helpers

Add at the bottom of `harness_suite.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crate_from_compile_error_standard() {
        let output = concat!(
            "error[E0599]: no function found\n",
            "   --> src/relay.rs:10:5\n",
            "error: could not compile `canon-llm-runtime` (lib test) ",
            "due to 1 previous error\n",
        );
        assert_eq!(
            crate_from_compile_error(output),
            Some("canon-llm-runtime".to_string())
        );
    }

    #[test]
    fn test_crate_from_compile_error_short_form() {
        let output = "error: could not compile `my-crate` due to 3 previous errors\n";
        assert_eq!(
            crate_from_compile_error(output),
            Some("my-crate".to_string())
        );
    }

    #[test]
    fn test_crate_from_compile_error_none_when_no_compile_error() {
        let output = "test foo ... FAILED\nfailures:\nfoo\n";
        assert_eq!(crate_from_compile_error(output), None);
    }

    #[test]
    fn test_compile_error_first_line_bracket_form() {
        let output = "warning: unused\nerror[E0599]: foo not found\n";
        let result = compile_error_first_line(output);
        assert!(result.as_deref().unwrap_or("").starts_with("error[E0599]"));
    }

    #[test]
    fn test_compile_error_first_line_plain_form() {
        let output = "warning: x\nerror: could not compile `foo`\n";
        let result = compile_error_first_line(output);
        assert!(result.as_deref().unwrap_or("").starts_with("error:"));
    }
}
```

---

## Expected result

After this fix, when `cargo test --workspace` fails to compile a crate the
harness suite will:

1. Extract `canon-llm-runtime` from `"could not compile \`canon-llm-runtime\`"`
2. Use the first compiler error line as the synthetic test name
3. Pass both to `run_harness_repair`, which feeds the compile error into
   `canon-harness-repair`
4. The repair agent reads the error, applies a patch to fix it, and the suite
   retries — eventually reaching a green workspace
