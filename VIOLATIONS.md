# Violations

## 1. Decision not structurally restricted to SemanticStateSummary (CRITICAL)
- Evidence:
  - decide_from_json(ctx: &RouteContext, ...) still accepts full RouteContext
  - Routing logic reads ctx.semantic_summary but has access to entire ctx
- Issue:
  - Violates canonical law: SemanticStateSummary must be the sole source of routing authority
  - Current implementation relies on discipline, not enforcement
- Required fix:
  - Change decision signature to accept SemanticStateSummary directly
  - Remove RouteContext from decision interface

## 2. SemanticStateSummary not isolated as authority boundary (HIGH)
- Evidence:
  - RouteContext contains mixed state (journal, tool results, verifier outputs, etc.)
  - semantic_summary is embedded, not isolated
- Issue:
  - Non-semantic fields remain accessible to decision logic
  - Future regressions are likely
- Required fix:
  - Enforce architectural boundary: decision = f(SemanticStateSummary)
  - Prevent decision code from accessing RouteContext

## 3. RouteController still present in decision interface (MEDIUM)
- Evidence:
  - decide_from_json includes RouteController parameter
  - Not used in current logic
- Issue:
  - Suggests potential for non-semantic influence path
- Required fix:
  - Remove RouteController from decision unless proven semantic-safe

## 4. Semantic source population not verified (HIGH)
- Evidence:
  - No proof semantic_summary is derived exclusively from observe pipeline
- Issue:
  - Could still be mutated from multiple sources
  - Violates single-source-of-truth invariant
- Required fix:
  - Verify semantic_summary is populated only from canonical observation events
  - Enforce immutability or controlled update path

## 5. System not fully spec-compliant
- Evidence:
  - Decision is semantically driven in implementation but not enforced at type level
- Issue:
  - Lacks hard guarantees required by spec
- Required fix:
  - Enforce SemanticStateSummary-only routing at type level
