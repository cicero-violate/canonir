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

### Inspect solver chain
```bash
perl -0777 -ne '...' canon-analyzer/src/solver/mod.rs  # extract_function("solve", ...)
```

### Inspect dep_solver
```bash
cat canon-analyzer/src/solver/dep_solver.rs
```

### Inspect emit_file (h1 removed — file.rs is now pure projection)
```bash
perl -0777 -ne '...' canon-projection/src/emit/file.rs  # extract_function("emit_file", ...)
```

### Inspect use_solver injection logic
```bash
perl -0777 -ne '...' canon-analyzer/src/solver/use_solver.rs  # extract_function("solve", ...)
```

### Inspect visibility_solver repairs
```bash
perl -0777 -ne '...' canon-analyzer/src/solver/visibility_solver.rs  # extract_function("solve", ...)
```

### Inspect build_plan (reads Crate.dependencies directly)
```bash
perl -0777 -ne '...' canon-projection/src/layout/mod.rs  # extract_function("build_plan", ...)
```

### Find all Use node construction sites
```bash
rg "CanonNodeKind::Use\s*\{" canon-analyzer/src/solver/use_solver.rs canon-capture/src/canon_assemble.rs -n
```

### Find Crate node construction (dependencies field)
```bash
rg "CanonNodeKind::Crate" canon-capture/src/canon_assemble.rs -n -A 5
```

### Verify no heuristics remain in emitter
```bash
rg "contains\|format!\|strip_prefix\|replace\|inject\|HashSet" \
  canon-projection/src/emit/file.rs \
  canon-projection/src/emit/fmt.rs \
  canon-projection/src/emit/items.rs \
  canon-projection/src/emit/impls.rs \
  canon-projection/src/layout/mod.rs -n
```
