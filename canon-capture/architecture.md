canon-capture/
│
├── lib.rs
├── index.rs
├── norm.rs
│
├── capture/
│   ├── mod.rs
│   ├── pipeline.rs        // top-level orchestration
│   ├── engine.rs          // rule dispatcher
│   ├── rules.rs           // declarative RuleSpec
│   ├── relations.rs       // relation templates only
│   ├── fragments.rs       // CanonFragment + builders
│   │
│   ├── mir/
│   │   ├── mod.rs
│   │   ├── lower.rs       // mir_body_structural (CFG walker)
│   │   ├── patterns.rs    // MirPattern table
│   │   ├── guard.rs       // structural_guard logic
│   │   └── resolver.rs    // LocalNameResolver
│   │
│   ├── validate/
│   │   ├── mod.rs
│   │   └── structural.rs  // emission invariants
│   │
│   └── helpers.rs         // type + generics mapping
