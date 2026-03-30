# HTTP Routing Engine with Middleware Pipeline and Coverage Discovery

This project implements a Rust-based HTTP routing and middleware execution engine that simulates request handling, route matching, and layered middleware processing, along with a coverage discovery system that identifies untested routing paths, middleware interactions, and edge-case request scenarios. It is interesting because HTTP routing systems involve pattern matching, layered execution, and branching logic that can create complex and subtle behavior across many combinations of routes and middleware.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/http-router-coverage`

## Requirements

1. Implement a Rust binary crate organized into modules such as `request`, `response`, `router`, `route`, `matcher`, `pattern`, `method`, `middleware`, `pipeline`, `handler`, `context`, `runtime`, `executor`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design HTTP request and response models including method, path, headers, query parameters, and body.
3. Implement a routing system supporting static paths, parameterized paths (e.g., `/users/:id`), and wildcard matching.
4. Build a route matcher that selects the correct handler based on HTTP method and path pattern with precedence rules.
5. Implement a middleware pipeline where middleware can modify requests/responses, short-circuit execution, or pass control to the next layer.
6. Support chaining and ordering of middleware with explicit execution flow control.
7. Handle edge cases such as ambiguous routes, missing parameters, unsupported methods, deeply nested middleware, and malformed requests.
8. Provide a CLI using `clap` to define routes and middleware via JSON/YAML, simulate HTTP requests, and inspect responses.
9. Create a trace system that records route matching decisions, middleware execution order, handler invocation, and response generation.
10. Build a coverage tracking system that records which routes, patterns, middleware paths, and branching behaviors have been exercised.
11. Develop an analysis module that identifies untested scenarios such as rare route overlaps, middleware short-circuit cases, parameter edge cases, and conflicting patterns, and implement a request generator that produces synthetic HTTP requests targeting uncovered behaviors.
12. Include reporting features such as route hit counts, middleware execution statistics, coverage summaries, and uncovered scenarios, ensuring the implementation spans at least 800 lines of Rust code across modules and compiles successfully with `cargo check`.