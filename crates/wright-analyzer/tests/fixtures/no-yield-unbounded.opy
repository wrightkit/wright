# Positive fixture for the while-without-wait rule (issue #103):
# obviously-unbounded boundedness class.
#
# A `while true:` loop with no wait: the condition is statically true and the
# modeled WIR has no break/goto action, so the loop never terminates on its
# own and repeats without yielding while the rule is active.
#
# Wright-authored fixture (AGPL-3.0-or-later); no third-party material.
#
# Expected: exactly one `while-without-wait` finding on the `While` action,
# boundedness `obviously-unbounded`, severity warning.

globalvar counter = 0

rule "unbounded no-yield loop":
    @Event global
    while true:
        counter += 1
