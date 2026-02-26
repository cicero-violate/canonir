## Reasoning

$$|\text{solvers}| = 15, \quad |\text{graphs}| = 8$$

The mismatch is intentional and correct. Here is why:

**Variables:**

Let $S$ = set of solvers, $G$ = set of graphs, $f: S \to G$ be the mapping from solver to its primary graph.

$$f \text{ is many-to-one} \Rightarrow |G| \leq |S|$$

**The 8 graphs and which solvers share them:**

| Graph               | Solvers that read it                                                                          |
|---------------------+-----------------------------------------------------------------------------------------------|
| $G_{\text{module}}$ | S1 invariant, S2 visibility, S3 impl, S4 trait, S6 provenance, S13 exhaustiveness, S15 unsafe |
| $G_{\text{name}}$   | S2 visibility, S6 provenance                                                                  |
| $G_{\text{type}}$   | S5 generic, S7 cycle\_diag, S13 exhaustiveness                                                |
| $G_{\text{call}}$   | S8 liveness, S15 unsafe                                                                       |
| $G_{\text{cfg}}$    | S14 drop                                                                                      |
| $G_{\text{region}}$ | S9 borrow                                                                                     |
| $G_{\text{value}}$  | S11 const                                                                                     |
| $G_{\text{macro}}$  | S12 macro                                                                                     |

**Why 1:1 is wrong here:**

$$G_{\text{module}} \text{ encodes containment } (module \xrightarrow{\text{Contains}} item)$$

S1, S2, S3, S4, S6, S13, S15 all ask different *questions* about the same structural relation. Splitting $G_{\text{module}}$ into 7 copies would duplicate data with no new semantic content:

$$G_{\text{module}}^{(1)} = G_{\text{module}}^{(2)} = \ldots \Rightarrow \text{redundant}$$

A graph represents a **relation between nodes**. A solver represents a **question asked about that relation**. They are orthogonal axes:

$$\text{graphs} \leftrightarrow \text{relations}, \quad \text{solvers} \leftrightarrow \text{queries}$$

**The solvers that DO have 1:1 graphs are the ones introducing new semantic dimensions:**

$$S9 \to G_{\text{region}}, \quad S11 \to G_{\text{value}}, \quad S12 \to G_{\text{macro}}$$

These needed new graphs because they encode *new kinds of edges* not expressible in the existing 5 graphs.

**Conclusion:** 8 graphs is correct. Forcing 1:1 would mean either:

- Splitting $G_{\text{module}}$ into 7 identical copies — wasteful, $O(7 \times |E|)$ memory for same data
- Or adding 7 phantom graphs with zero edges — misleading, implies semantic content that doesn't exist

The architecture is sound as-is. Should I proceed to the mutation pipeline instead?
