# Peer-to-Peer Gossip Protocol Simulator with Membership, Failure Detection, and State Dissemination

This project implements a peer-to-peer gossip protocol simulator in Rust that models how distributed systems propagate state across nodes using randomized communication. It includes membership management, failure detection, anti-entropy synchronization, and message dissemination strategies. The system simulates unreliable networks with delays, message loss, and partitions. This project is interesting because it combines distributed systems algorithms, probabilistic communication, simulation, and state convergence into a realistic and complex system inspired by protocols like SWIM and Cassandra gossip.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/gossip_protocol_simulator`

## Requirements

1. Implement a Rust binary crate structured into modules such as `node`, `cluster`, `membership`, `state`, `message`, `protocol`, `gossip`, `failure_detector`, `heartbeat`, `network`, `transport`, `scheduler`, `simulation`, `engine`, `cli`, and `errors`.
2. Design a node model with unique IDs, local state, membership list, and versioned metadata for gossip dissemination.
3. Implement a gossip protocol where nodes periodically select peers and exchange state information using push, pull, or push-pull strategies.
4. Build a membership system that tracks node join, leave, and suspected/failed states with versioning and conflict resolution.
5. Implement a failure detection mechanism (e.g., SWIM-style) using heartbeats, timeouts, and suspicion levels.
6. Simulate a network layer that introduces configurable latency, message loss, duplication, and partitions.
7. Implement anti-entropy synchronization to ensure eventual consistency across nodes even after message loss.
8. Support state merging strategies that resolve conflicts using version vectors or timestamps.
9. Build a scheduler that advances simulation time and triggers periodic gossip rounds and failure checks.
10. Provide observability features such as cluster convergence metrics, message counts, and node state summaries.
11. Provide a CLI using `clap` with commands like `simulate`, `partition`, `heal`, `status`, and `metrics`.
12. Integrate structured logging with `tracing` to trace gossip exchanges, membership changes, failure detection events, and convergence behavior, and ensure the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.