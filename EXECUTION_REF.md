## Agent Tooling Reference

### Inspect node variants (node.rs)
```bash
rg "Crate\s*\{|Use\s*\{|Module\s*\{" canon/src/node.rs -A 8
```

### Extract any function by name (brace-balanced)
```bash
perl -0777 -ne '
my $src = $_;
sub extract_function {
    my ($name, $file) = @_;
    while ($src =~ /(^\s*(pub\s+)?fn\s+$name[^{]*\{)/mg) {
        my $start = pos($src) - length($1);
        my $depth = 1; my $i = pos($src);
        while ($depth && $i < length($src)) {
            my $c = substr($src, $i, 1);
            $depth++ if $c eq "{"; $depth-- if $c eq "}"; $i++;
        }
        my $block = substr($src, $start, $i - $start);
        my @lines = split /\n/, $block;
        print "\n=== $file :: $name ===\n\n";
        printf "%5d  %s\n", $_+1, $lines[$_] for 0..$#lines;
        pos($src) = $i;
    }
}
extract_function("FUNCTION_NAME", $ARGV);
' path/to/file.rs
```

### Phase 4 — h1: Remove path injection from file.rs
Target: `emit_file` in `canon-projection/src/emit/file.rs` (lines 24–37, the string-scan inject block).
Work moves to `use_solver.rs` — inject std::path::Path, PathBuf, and local types via Use nodes with resolved `target`.
```bash
# Inspect current h1 injection block
perl -0777 -ne '...' canon-projection/src/emit/file.rs  # extract_function("emit_file", ...)
# Find all Use node construction sites
rg "CanonNodeKind::Use\s*\{" canon-analyzer/src/solver/use_solver.rs -n
# Find solver chain order
perl -0777 -ne '...' canon-analyzer/src/solver/mod.rs    # extract_function("solve", ...)
```

### Phase 6 — h6: Add dep_solver and wire into chain
Target: new `canon-analyzer/src/solver/dep_solver.rs`; update `solver/mod.rs`; update `layout/mod.rs` `build_plan`.
```bash
# Inspect current build_plan deps read (after g1 fix)
perl -0777 -ne '...' canon-projection/src/layout/mod.rs  # extract_function("build_plan", ...)
# Inspect solver chain
perl -0777 -ne '...' canon-analyzer/src/solver/mod.rs    # extract_function("solve", ...)
# Find Crate node construction in assemble (to see where dep_solver writes)
rg "CanonNodeKind::Crate" canon-capture/src/canon_assemble.rs -n -A 5
# Find path_intern usage (PathId interning pattern for dep paths)
rg "path_intern.intern" canon-capture/src/canon_assemble.rs -n
```
