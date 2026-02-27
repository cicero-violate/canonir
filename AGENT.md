RULES TO FOLLOW
If you are tempted to create a heuristic, it means there is a structural gap.The new solution lacks invariants. Therefore abort the job, and notify the user.

USEFUL INFORMATION
rustc compiler source code can be found in here, it is very useful
/workspace/ai_sandbox/canon/test_projects/rust_compiler_info/compiler

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

bat
- Use only as a last resort for manual inspection.
- Use when full-file visual context is required.
- Do not use bat for automated extraction or structured processing.

json
never open json files completely, use python

example
```bash
perl -0777 -ne '
my $src = $_;

sub extract_function {
    my ($name, $file) = @_;
    while ($src =~ /(^\s*(pub\s+)?fn\s+$name[^{]*\{)/mg) {
        my $start = pos($src) - length($1);
        my $depth = 1;
        my $i = pos($src);

        while ($depth && $i < length($src)) {
            my $c = substr($src, $i, 1);
            $depth++ if $c eq "{";
            $depth-- if $c eq "}";
            $i++;
        }

        my $block = substr($src, $start, $i - $start);

        if ($block =~ /flags::|vis_token|normalize_use_path|for_trait|UNSAFE|INLINE/) {
            my @lines = split /\n/, $block;
            print "\n=== $file :: $name ===\n\n";
            for my $j (0..$#lines) {
                printf "%5d  %s\n", $j+1, $lines[$j];
            }
        }

        pos($src) = $i;
    }
}

extract_function("dispatch_item", $ARGV);
extract_function("emit_node", $ARGV);
extract_function("emit_module", $ARGV);
extract_function("emit_impl", $ARGV);
' canon-projection/src/emit/items.rs \
  canon-projection/src/emit/impls.rs
```

USEFUL INFORMATION
rustc compiler source code can be found in here
/workspace/ai_sandbox/canon/test_projects/rust_compiler_info/compiler


