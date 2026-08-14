# Positive fixture for the while-without-wait rule (REQ-002): a `While`
# loop whose body tree contains no `wait` call.
#
# Corpus context: a corpus scan of the emitted workshop text found `While`
# loops only in overpy-cake (workshop line 46) and overpy-client-to-server
# (workshop line 54), and both contain `Wait(0.016, ...)`; there is no corpus
# positive evidence. This synthetic fixture is the documented validation:
# the trigger is a statically exact structural fact.
#
# Wright-authored fixture (AGPL-3.0-or-later); no third-party material.
#
# Expected: exactly one `while-without-wait` finding on the `While` action.

globalvar counter = 0

rule "waardless while loop":
    @Event global
    while true:
        counter += 1
