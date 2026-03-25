# HTTP Reverse Proxy with Load Balancing, Middleware Pipeline, and Circuit Breaker

This project implements a high-performance HTTP reverse proxy in Rust that routes incoming requests to backend services using configurable load balancing strategies, middleware processing, and resilience features such as circuit breakers and retries. It supports request/response transformation, logging, rate limiting, and health checks. The system models real-world API gateways and proxies like NGINX or Envoy in a simplified but modular form. This project is interesting because it combines networking, async I/O, middleware design, fault tolerance, and traffic management into a critical infrastructure component.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/http_reverse_proxy`

## Requirements

1. Implement a Rust binary crate structured into modules such as `server`, `router`, `backend`, `load_balancer`, `strategy`, `middleware`, `pipeline`, `request`, `response`, `circuit_breaker`, `health`, `retry`, `rate_limit`, `config`, `cli`, and `errors`.
2. Build an HTTP server using `hyper` or `axum` that accepts incoming requests and forwards them to configured backend services.
3. Implement routing logic that maps incoming paths/hosts to backend clusters.
4. Design multiple load balancing strategies such as round-robin, least-connections, and random selection.
5. Implement a middleware pipeline that allows request/response processing (e.g., logging, header modification, authentication hooks).
6. Implement a circuit breaker that tracks backend failures and temporarily disables unhealthy backends.
7. Add retry logic with configurable policies (max retries, backoff strategies) for failed requests.
8. Implement backend health checks (active and/or passive) to determine availability.
9. Add rate limiting per client or route using token bucket or sliding window algorithms.
10. Support configuration loading via JSON or YAML (`serde`) for defining routes, backends, and policies.
11. Provide a CLI using `clap` with commands like `run`, `reload-config`, `status`, and `stats`.
12. Integrate structured logging with `tracing` to trace request flow, load balancing decisions, middleware execution, failures, and recovery behavior, and ensure the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.