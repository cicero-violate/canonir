| Goal                                         | Command                                     |                 |                     |
|----------------------------------------------+---------------------------------------------+-----------------+---------------------|
| Find all functions over complexity threshold | `rustc -Z unpretty=mir \                    | rg "bb[0-9]+" \ | wc -l` per function |
| Find duplicate code patterns                 | `rg` for structural repetition across files |                 |                     |
| Find dead code                               | `rustc -W dead-code`                        |                 |                     |
| Find unsafe blocks                           | `rg "unsafe \{"`                            |                 |                     |
| Find unwrap/expect calls                     | `rg "\.unwrap\(\)\                          | \.expect\("`    |                     |
| Find missing error propagation               | `-Z unpretty=mir` on error paths            |                 |                     |
| Find type complexity                         | `-Z unpretty=hir` on type aliases           |                 |                     |

| What model queries            | rustc flag                                       |
|-------------------------------+--------------------------------------------------|
| MIR for one function          | `--emit=mir` + filter by function name with `rg` |
| Type of an expression         | `-Z unpretty=mir`                                |
| Borrow check region inference | `-Z dump-mir=nll`                                |
| Dataflow liveness             | `-Z dump-mir=dataflow`                           |
| Optimised vs unoptimised MIR  | `-Z mir-opt-level=0` vs `3`                      |
| Specific MIR pass output      | `-Z dump-mir=ConstProp`                          |
| HIR                           | `-Z unpretty=hir`                                |
| THIR                          | `-Z unpretty=thir-flat`                          |

