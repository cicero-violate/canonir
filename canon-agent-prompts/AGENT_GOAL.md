# Entity Component System (ECS) Game Engine Core with Scheduling and Query System

This project implements a core Entity Component System (ECS) engine in Rust, providing a flexible and high-performance architecture for building games and simulations. It includes entity management, component storage, system scheduling, and query execution over large datasets. The engine supports parallel system execution and archetype-based storage for cache-efficient processing. This project is interesting because it combines data-oriented design, scheduling, memory layout optimization, and query systems into a modern game engine foundation.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/ecs_engine`

## Requirements

1. Implement a Rust binary crate structured into modules such as `entity`, `component`, `storage`, `archetype`, `world`, `query`, `system`, `scheduler`, `resource`, `executor`, `cli`, and `errors`.
2. Design an entity system that generates unique entity IDs and supports creation, deletion, and reuse of IDs.
3. Implement component storage using archetype-based layout grouping entities by component sets for efficient iteration.
4. Build a type-safe component registration system using generics and `TypeId` to store heterogeneous components.
5. Implement a query system that allows filtering entities by component combinations and iterating efficiently over matching archetypes.
6. Design a system abstraction where systems define logic over queries and can access shared resources.
7. Implement a scheduler that determines execution order based on system dependencies and resource access patterns.
8. Support parallel execution of systems using thread pools (e.g., via `rayon`) while avoiding data races.
9. Implement resource management for global state (e.g., time, configuration) accessible by systems.
10. Provide change detection or event tracking for components to allow systems to react only to updates.
11. Provide a CLI using `clap` with commands like `run`, `benchmark`, `inspect-world`, and `profile`.
12. Integrate structured logging with `tracing` to trace entity lifecycle, system execution, scheduling decisions, and query performance, and ensure the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.