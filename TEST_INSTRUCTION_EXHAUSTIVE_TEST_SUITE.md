GOAL:
Build an exhaustive test suite for semantic edit capabilities:
- rename_symbol
- move_symbol
- import resolution
- module creation

REQUIREMENTS:
1. Generate test cases covering:
   - simple cases
   - cross-module references
   - shadowing / duplicate symbols
   - trait + impl interactions
   - failure / invalid cases

2. For each test:
   - initial code state
   - semantic action applied
   - expected resulting graph + code
   - compile must pass

3. Add invariant checks:
   - no duplicate definitions
   - all references resolved
   - graph consistency preserved

4. Run tests automatically:
   - cargo check / build
   - fail on any mismatch

5. Output:
   - test files
   - execution results
   - summary of coverage gaps

CONSTRAINT:
- prefer semantic actions over text patches
- do not use heuristic fixes

FINAL STEP:
Verify all tests pass before completion.
