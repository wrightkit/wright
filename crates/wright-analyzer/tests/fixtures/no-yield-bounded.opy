# Positive fixture for the while-without-wait rule (issue #103):
# statically-bounded boundedness class.
#
# Rule 1 is the real-consumer repro from wrightkit/agent-lab#68
# (`loop-waitless.opy`) verbatim: a finite 10-iteration counter loop with no
# wait, classified as `statically-bounded` (a finite counter loop), NOT as an
# unbounded hazard.
#
# Rule 2 pins the subtract direction: `while n > 0: n -= 1` decrements the
# compared variable toward the literal bound.
#
# Rule 3 pins sound nested loops: a nested counter `while k < 3: k += 1` is
# provably finite, so the outer `while m < 10: m += 1` stays statically
# bounded.
#
# Rule 4 pins a sound nested `For Global Variable` with a non-zero literal
# step whose body does not write the loop variable: the outer loop stays
# statically bounded.
#
# Wright-authored fixture (AGPL-3.0-or-later); no third-party material.
#
# Expected: exactly five `while-without-wait` findings (loopCount, n, m, k,
# p), all boundedness `statically-bounded`, severity info.

globalvar loopCount
globalvar n
globalvar m = 0
globalvar k = 0
globalvar p = 0
globalvar q = 0
globalvar x = 0

rule "waitless loop":
    @Condition getTotalTimeElapsed() > 5

    loopCount = 0
    while loopCount < 10:
        loopCount += 1

rule "decrement counter loop":
    @Event global

    n = 10
    while n > 0:
        n -= 1

rule "nested sound counter loop":
    @Event global

    m = 0
    while m < 10:
        m += 1
        k = 0
        while k < 3:
            k += 1

rule "nested sound for loop":
    @Event global

    p = 0
    while p < 5:
        p += 1
        for q in range(3):
            x += 1
