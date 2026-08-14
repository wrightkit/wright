# Negative fixture for the repeated-value rule (REQ-001). Every case below
# must produce NO `repeated-value` finding:
#
#   1. identical expressions used in different rules
#   2. identical expressions outside any loop
#   3. duplicated trivial literals/vectors inside a loop (no Call anywhere)
#   4. non-identical sibling expressions in a loop
#   5. duplication crossing a nested-loop boundary (each loop is its own scope)
#   6. duplication in `For Global Variable` bounds (bounds are excluded from
#      the loop scope)
#   7. duplicated single-call expressions inside a loop (a bare `Value In
#      Array` read; a single-call predicate with plain operands) — the
#      non-trivial filter requires at least two `Call` nodes per occurrence
#
# Wright-authored fixture (AGPL-3.0-or-later); no third-party material.
#
# Expected: zero `repeated-value` findings.

globalvar I = 0
globalvar J = 0
globalvar base = vect(1, 2, 3)
globalvar target = vect(4, 5, 6)
globalvar boxes = [vect(0, 0, 0), vect(1, 0, 0)]

rule "identical expressions in different rules (a)":
    @Event global
    debug(distance(base, target))

rule "identical expressions in different rules (b)":
    @Event global
    debug(distance(base, target))

rule "identical expressions outside any loop":
    @Event global
    debug(distance(base, target))
    debug(distance(base, target))

rule "duplicated trivial values inside a loop":
    @Event global
    for I in range(3):
        debug(1)
        debug(1)
        debug(vect(1, 2, 3))
        debug(vect(1, 2, 3))

rule "non-identical sibling expressions in a loop":
    @Event global
    for I in range(3):
        debug(distance(base, target))
        debug(distance(target, base))

rule "duplication crossing a nested-loop boundary":
    @Event global
    for I in range(3):
        debug(distance(base, target))
        for J in range(2):
            debug(distance(base, target))

rule "duplication in for-loop bounds":
    @Event global
    for I in range(len(boxes), len(boxes)):
        debug(I)

rule "duplicated single-call array reads inside a loop":
    @Event global
    for I in range(3):
        debug(boxes[I])
        debug(boxes[I])

rule "duplicated single-call predicate inside a loop":
    @Event global
    for I in range(3):
        debug(distance(base, target))
        debug(distance(base, target))
