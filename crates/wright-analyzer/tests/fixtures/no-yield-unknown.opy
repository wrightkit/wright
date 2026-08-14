# Positive fixture for the while-without-wait rule (issue #103):
# unknown boundedness class.
#
# Rule 1: a data-dependent condition comparing two variables (`a < b`); the
# compared variable `a` is not modified in the body, so there is no static
# counter pattern and boundedness is not decidable.
#
# Rule 2: the condition compares a variable against a literal, but the modify
# that would progress the loop is nested inside an `if` (conditional
# progress), so the loop's boundedness is not provable.
#
# Rule 3 (away-writer): a toward-modify is present, but a second direct child
# moves the compared variable away from the bound (`v1 += 1; v1 -= 2`), so
# progress is not provable.
#
# Rule 4 (conditional reset): a direct child conditionally writes the compared
# variable (`if flag: v2 = 0`), so progress is not provable.
#
# Rule 5 (subroutine writer): a direct child calls a subroutine whose body
# writes the compared variable, so progress is not provable.
#
# Rule 6 (non-literal step): the additional modify uses a non-literal step
# (`v4 += other`), so progress is not provable.
#
# Rule 7 (nested-loop writer): a nested `while true:` writes the compared
# variable; the OUTER loop's progress is not provable, while the inner loop is
# itself an obviously-unbounded no-yield finding.
#
# Rule 8 (nested non-terminating inner loop): a nested `while true:` does NOT
# write the compared variable; even so the outer loop never completes an
# iteration, so its boundedness is not provable (the inner loop is itself an
# obviously-unbounded no-yield finding).
#
# Rule 9 (nested loop inside an If branch): a `for` with a zero literal step
# (`range(0, 4, 0)`) is non-terminating per Workshop engine semantics and is
# nested inside an `if`; the outer loop's boundedness is not provable.
#
# Wright-authored fixture (AGPL-3.0-or-later); no third-party material.
#
# Expected: eleven `while-without-wait` findings total: the nine outer/data
# loops are `unknown` (warning), and the two inner `while true:` loops of
# rules 7 and 8 are `obviously-unbounded` (warning).

globalvar a = 0
globalvar b = 10
globalvar x = 0
globalvar n = 0
globalvar v1 = 0
globalvar v2 = 0
globalvar v3 = 0
globalvar v4 = 0
globalvar v5 = 0
globalvar v6 = 0
globalvar v7 = 0
globalvar q = 0
globalvar other = 3
globalvar flag = false

def bumpV3():
    v3 += 1

rule "data-dependent condition loop":
    @Event global
    while a < b:
        x += 1

rule "conditional progress loop":
    @Event global
    while n < 10:
        if a < b:
            n += 1

rule "away writer":
    @Event global
    while v1 < 10:
        v1 += 1
        v1 -= 2

rule "conditional reset":
    @Event global
    while v2 < 10:
        v2 += 1
        if flag:
            v2 = 0

rule "subroutine writer":
    @Event global
    while v3 < 10:
        v3 += 1
        bumpV3()

rule "non literal step":
    @Event global
    while v4 < 10:
        v4 += 1
        v4 += other

rule "nested loop writer":
    @Event global
    while v5 < 10:
        v5 += 1
        while true:
            v5 += 1

rule "nested non terminating inner loop":
    @Event global
    while v6 < 10:
        v6 += 1
        while true:
            x += 1

rule "nested zero step for inside if":
    @Event global
    while v7 < 10:
        v7 += 1
        if flag:
            for q in range(0, 4, 0):
                x += 1
