Let me build this out formally and completely.

---

## Variables

Let $s_i \in [0,1]$ = signal value for signal $i$

Let $\tau^-_i, \tau^+_i$ = lower and upper thresholds for signal $i$

Let $\mathcal{D}_i$ = decision set available at signal $i$

Let $\omega_i$ = urgency weight of signal $i$

Let $\Phi(s_i)$ = decision function mapping signal value to ordered action range

---

## Latent Equations

**Decision as a function of signal position in range:**

$$\Phi(s_i) = \mathcal{D}_k \quad \text{where} \quad k = \left\lfloor \frac{s_i}{\tau_{\text{step}}} \right\rfloor$$

**Composite urgency score across all signals:**

$$\Omega = \sum_{i} \omega_i \cdot \mathbb{1}[s_i \notin [\tau^-_i, \tau^+_i]]$$

**Action priority ordering:**

$$\mathcal{A}_{\text{selected}} = \arg\max_{a \in \mathcal{D}} \left( \omega_i \cdot |s_i - \tau^*_i| \right)$$

where $\tau^*_i$ is the ideal target value for signal $i$.

---

## Full Signal → Decision Range Table

Now let me visualize this properly.Each signal now exposes a full decision range rather than a single binary reaction. The key design principle is that every signal has four zones — critical, warning, caution, and optimal — and each zone maps to a *different set* of actions with escalating urgency.

A few things to note about the structure:

`progress_score` combined with `novelty_score` is your loop detector. When both are in their critical zones simultaneously, that is the composite $\mathcal{L} = 1$ condition — the agent is spinning. Neither signal alone is sufficient to diagnose a loop; you need both.

`execution_risk` and `confidence` form your safe execution gate $\mathcal{G}_{\text{safe}}$. If either crosses into critical, the `block` action fires before any `execute` is permitted.

`termination_readiness` is intentionally inverted — a very high score combined with low `verification_coverage` is *more* dangerous than a low score, because premature convergence is a silent failure mode. The table reflects that: high readiness + low coverage still routes to `validate` and `block`, not `execute`.

You can filter by zone to see all critical-condition signals at once — useful for building your priority stack in the router.
