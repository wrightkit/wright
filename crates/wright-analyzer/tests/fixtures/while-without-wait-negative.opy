# Negative fixture for the while-without-wait rule (REQ-002). Every case
# below must produce NO `while-without-wait` finding:
#
#   1. `While` with a literal wait in the body
#   2. `While` with a computed (non-literal) wait duration
#   3. `While` whose wait is nested inside an `If` in the body
#   4. `For Global Variable` loop with no wait (never flagged)
#
# Wright-authored fixture (AGPL-3.0-or-later); no third-party material.
#
# Expected: zero `while-without-wait` findings.

globalvar I = 0
globalvar counter = 0
globalvar someDuration = 0.1
globalvar flag = false

rule "while with literal wait":
    @Event global
    while true:
        wait(0.016)
        counter += 1

rule "while with computed wait":
    @Event global
    while true:
        wait(someDuration)
        counter += 1

rule "while with wait nested in if":
    @Event global
    while true:
        if flag:
            wait(0.1)
        else:
            counter += 1

rule "for loop without wait":
    @Event global
    for I in range(4):
        counter += 1
