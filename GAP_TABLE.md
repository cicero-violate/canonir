| Gap                                     | Location            | Effect                                        | Correct Fix                                    |
|-----------------------------------------+---------------------+-----------------------------------------------+------------------------------------------------|
| No `QualifiedPath` node                 | `node.rs`           | Path strings accumulate in intern, no grammar | Structured path node with segments             |
| No restricted `Visibility` node         | `node.rs`           | `pub(in path)` silently dropped               | `Visibility` node or `flags + path_id`         |
| No `ExternRef` / `PathRef` node         | `node.rs`           | dep_solver text-scans                         | Body path ref node at capture                  |
| No `AssocType` node                     | `node.rs`           | Trait/impl assoc types lost                   | Dedicated node kind                            |
| No `AssocConst` node                    | `node.rs`           | Trait/impl assoc consts lost                  | Dedicated node kind                            |
| `CfgOp` missing 5+ variants             | `node.rs`           | Most body ops become `Raw`                    | Add method call, field, closure, etc.          |
| No `Pattern` node                       | `node.rs`           | Destructuring lost                            | Pattern node with variants                     |
| `WherePred` missing lifetime bounds     | `node.rs`           | Lifetime predicates lost                      | Add `LifetimePred` variant                     |
| No `AssocItem` edge                     | `edge.rs`           | Trait/impl structure partial                  | New edge kind                                  |
| No `Instantiates` edge                  | `edge.rs`           | Generic usage invisible to type graph         | New edge kind                                  |
| `ImplRef` in wrong graph                | `edge.rs` + routing | impl_solver/trait_solver may miss it          | Route to `G_type`                              |
| No `Reexports` edge                     | `edge.rs`           | Re-export structure not in graph              | New edge kind                                  |
| `G_cfg` dual-authority for successors   | `node.rs` + `ir.rs` | CFG edges redundant/inconsistent              | Remove inline `u32` successors, use graph only |
| `path_intern` no normalization contract | `intern.rs`         | Same path = multiple PathIds                  | Normalizing `intern_path`                      |
| `name_intern` conflates namespaces      | `intern.rs`         | NameId is ambiguous                           | Separate interners or tagged IDs               |
| `TypeKey` hashes `Debug` string         | `ir.rs`             | Type dedup unreliable                         | Structural recursive hash                      |
| No unresolved/resolved type distinction | `node.rs`           | Solvers cannot mark resolution state          | `Unresolved(PathId)` vs `Resolved(CanonId)`    |
| liveness_solver prunes externally-called functions | `canon-analyzer/src/solver/liveness_solver.rs` | E0425 missing emitted callees (e.g. `build_repomap`) | Keep any function whose NodeId appears as a callee in emitted `CfgOp::Call` |
