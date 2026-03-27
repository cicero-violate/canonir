# Finite Element Method (FEM) Solver for 2D Structural Analysis with Mesh Generation and Numerical Integration

This project implements a 2D Finite Element Method (FEM) solver in Rust for structural analysis problems such as stress and displacement in elastic materials. It supports mesh generation, element assembly, boundary conditions, and solving linear systems derived from physical models. The system includes numerical integration and matrix assembly, enabling simulation of real-world engineering problems. This project is interesting because it combines numerical methods, linear algebra, geometry, and simulation into a scientific computing system.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/fem_solver`

## Requirements

1. Implement a Rust binary crate structured into modules such as `mesh`, `node`, `element`, `geometry`, `shape_function`, `integration`, `quadrature`, `material`, `stiffness`, `assembly`, `matrix`, `solver`, `boundary`, `load`, `engine`, `cli`, and `errors`.
2. Design a mesh representation supporting nodes and elements (e.g., triangular or quadrilateral elements) with connectivity information.
3. Implement mesh generation utilities for simple geometries (rectangular grids, triangular meshes).
4. Define shape functions for supported element types and compute their derivatives.
5. Implement numerical integration (Gaussian quadrature) for computing element matrices.
6. Build element stiffness matrix computation based on material properties and geometry.
7. Assemble a global stiffness matrix from element contributions using sparse matrix structures.
8. Implement boundary condition handling (Dirichlet and Neumann conditions) and load application.
9. Solve the resulting linear system using a basic solver (e.g., Gaussian elimination or iterative methods like Conjugate Gradient).
10. Compute derived quantities such as displacement fields and stress distribution.
11. Provide a CLI using `clap` with commands like `generate-mesh`, `solve`, `inspect`, and `export`.
12. Integrate structured logging with `tracing` to trace mesh generation, matrix assembly, integration steps, solver iterations, and result computation, and ensure the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.