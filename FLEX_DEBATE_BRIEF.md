# FLEX_DEBATE_BRIEF.md — the PR 3 third-party work order

Status: `CONVENED 2026-10-19 — implementation unlocks when every
obligation below returns a named verdict`. Subject: `FLEX_MAPPING.md`
(amended), `PLAN.md` PR 3 (rewritten). This brief is the work order;
the documents carry the substance. Rule: agreement is per obligation,
BY NAME — a general assent is not agreement (PLAN.md decision log,
2026-10-19).

## Obligation (a) — source spot-check

Verify the five skeleton claims (FLEX_MAPPING §2.1) FROM the fetched
engine sources, not from the mapping document's paraphrase. Each claim
returns CONFIRMED or REFUTED with file:line from the source itself.

| # | Claim | Blink anchor | Gecko anchor |
|---|---|---|---|
| 1 | Item formation happens BELOW the algorithm (anonymous runs pre-wrapped; abs-pos/none excluded from item slots) | `layout_block.cc:147-171`; `flex_child_iterator.cc:18-24` | `nsCSSFrameConstructor.cpp:8134` (+ ~8180 whitespace drop) |
| 2 | Content-box base; automatic minimum FOLDED INTO the hypothetical clamp | `flex_item.h:47-48,104-112`; `flex_layout_algorithm.cc:1060-1178` | `nsFlexContainerFrame.cpp:660-698` |
| 3 | §9.7 seed at hypothetical; freeze ONE violation class per iteration by signed total | `line_flexer.cc:26-43,90-181` | `nsFlexContainerFrame.cpp:3050-3176` |
| 4 | Cross sizes determined AFTER main; `CanMainSizeInfluenceCrossSize` flag semantics | `flex_layout_algorithm.cc:1524-1612` | `nsFlexContainerFrame.cpp:2546-2600,4611-4645` |
| 5 | Negative free-space alignment: 0-clamp / safe collapse, never unsafe center | `flex_layout_algorithm.cc:1660-1715` | `nsFlexContainerFrame.cpp:3553-3581,3736-3762` |

## Obligation (b) — counterexample hunt

Attack, from the enumerated corpus (the 424-proof suite, the two-axis
enumeration, the composition families), for a FAILING scenario against:

- the §3.2 run-aggregation cursors: heterogeneous fonts inside one run;
  the widest unbreakable segment spanning a node boundary (adjacent
  spans, no intervening space); a whitespace-only run under `pre-wrap`
  (must form an item) vs under `normal`/`pre-line` (must drop);
- the refold bound: nested `main_influences_cross` flags (text inside a
  nested flex item inside a shrink context) — is once-per-level
  sufficient, and is the level count what the metric would observe?

Verdict per attack: the scenario, expected, actual — or "none found"
with the search scope stated.

## Obligation (c) — classification audit

Attack every §1 NO/PARTIAL row for a hidden structural dependency that
would make an in-place pin-flip landing dishonest. REQUIRED specific
probe (the architect's countersign, PLAN.md decision log 2026-10-19):
the paint-order NO classification assumes the order-modified sequence
is consumable by paint TODAY — `line_collection_items` exists
(`flex.rs:177/388/1639`), but whether the solve's sequence survives
past relayout to paint time is unverified. Verdict per row: holds, or
reclassified with the dependency named.

## Obligation (d) — budget sign-off

Confirm: (i) the refold budget (FLEX_MAPPING §3.4) is MEASURABLE on the
pinned text-heavy scenario named there; (ii) the construction bound
(total intrinsic measurements per dirty update ≤ 2× the up-fold count,
once per nesting level) holds; (iii) the within-update memo fallback
(context-owned, keyed by `(node, resolved width)`, reset per update) is
designed rather than improvised and does not violate the
no-speculative-caches doctrine. Then RATIFY the fraction ceiling for
the pinned scenario (starting position: ≤ 50% of measured flex nodes)
or set the number with rationale.

## Obligation (e) — kill-criterion adjudication

Confirm the restated criterion (PLAN.md PR 3, question 5) is
falsifiable in both directions, then adjudicate it against the doctrine
evidence — the recursion and allocation inventory in the hot path:
per-container `Vec<MeasuredFlexItem>` (`flex.rs:406-434`), the
`HashMap` re-join (`flex.rs:1894-1923`), the recursive auto-min walk
(`flex.rs:1252-1367`), the staged double-layout of every child
(`flex.rs:2365-2480`). Verdict: the criterion FIRES (the restructure
dies — name which branch and the evidence) or does not fire.

## The debate's required outputs

1. The per-obligation verdicts (a)–(e), each named.
2. **Sequencing pulls:** which §1 NO/PARTIAL rows, if any, pull forward
   as in-place pin-flip landings on the current solver (accepting
   double work), each recorded in FLEX_MAPPING §4. None by default.
3. **The refold budget number** (obligation d).
4. **Grid's dated arc decision** (PLAN.md PR 3, question 6 — REQUIRED;
   the recorded recommendation: enter immediately after the fix
   sequence, before PR 4).

Implementation unlocks at FLEX_MAPPING §4 step 1 when all four outputs
are recorded in PLAN.md's decision log.
