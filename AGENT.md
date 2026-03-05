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
~/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/
/workspace/git_repos/cargo

## CODING STYLE

1. **Branching**		: Minimize `if` / `match` / `loop`; replace with table dispatch, iterators, and predicate functions.  
2. **Dispatch**			: Prefer static fn-pointer or index tables over match chains; one branch to index, zero inside.  
3. **Iteration**		: Use library algorithms (`topological_sort`, `scc`, `reachability`) instead of manual traversal loops.  
4. **Graph Execution**	: DAG determines execution order; nodes are pure kernels rather than stateful procedures.  
5. **GPU Offload**		: Push BFS, reachability, scheduling, and SCC to CUDA kernels when available.  
6. **Purity**			: Kernel functions are deterministic and side-effect-free; all inputs are explicit arguments.  
7. **Dataflow**			: Structure computation as transformations through a pipeline rather than nested control flow.  
8. **Error Handling**	: Prefer `Result` chaining and combinators (`and_then`, `map`, `then_some`) over guard branches.  
9. **Deduplication**	: Extract shared logic (retry, edge application, ID coercion) into single reusable helpers.  
10. **Readability**		: Code should be flat, composable, and index-addressable for easy human and LLM reasoning.  
11. **Invariants**		: Core structural properties (unique IDs, DAG validity, reachability) are enforced explicitly.  
12. **Validation**		: Check invariants at module boundaries (`build_graph`, `apply_edges`) instead of scattered guards.  
13. **Fail Fast**		: Invalid states return errors immediately rather than branching into recovery logic.  
14. **Index Integrity**	: Graph index maps must remain synchronized with node storage and updated atomically.  
15. **Determinism**		: Program correctness derives from invariant preservation across pipeline stages.
