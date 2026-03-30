# Event-Driven Simulation Engine with Entity Systems and Coverage Discovery

This project implements a Rust-based event-driven simulation engine using an entity-component-system (ECS) architecture, supporting dynamic entities, systems, and event propagation, along with a coverage discovery system that identifies untested interactions between entities, systems, and event flows. It is interesting because ECS architectures combine data-oriented design, scheduling, and reactive event handling, resulting in complex emergent behaviors and execution paths.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/ecs-sim-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `entity`, `component`, `storage`, `archetype`, `world`, `system`, `scheduler`, `event`, `dispatcher`, `query`, `filter`, `runtime`, `time`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design an entity-component-system architecture where entities are IDs, components are typed data, and systems operate over queries of components.
3. Implement efficient component storage using archetypes or sparse sets to allow fast iteration and mutation.
4. Build a query system that allows systems to retrieve entities matching component filters with mutable and immutable access rules enforced.
5. Develop a scheduler that determines system execution order based on declared read/write dependencies between components.
6. Implement an event system where systems can emit and consume events, with support for queues, broadcast, and targeted delivery.
7. Support simulation time progression with ticks and optional delta time, allowing systems to run at different frequencies.
8. Handle edge cases such as entity deletion during iteration, conflicting mutable borrows, empty queries, cyclic system dependencies, and event storms.
9. Provide a CLI using `clap` to run simulations, load scenarios from JSON, step through ticks, and inspect entity/component states.
10. Create a trace system that records system execution order, component accesses, entity mutations, and event propagation.
11. Build a coverage tracking system that records which system interactions, query paths, event flows, and mutation patterns have been exercised.
12. Develop an analysis module that identifies untested scenarios such as rare component combinations, conflicting system schedules, deep event chains, and edge-case entity lifecycles, and implement a scenario generator that produces synthetic worlds and event sequences targeting uncovered behaviors, with reporting features including system execution metrics, entity counts, coverage summaries, and uncovered scenarios, ensuring the implementation spans at least 800 lines of Rust code across modules and compiles successfully with `cargo check`.