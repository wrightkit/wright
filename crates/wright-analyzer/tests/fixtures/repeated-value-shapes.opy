# Positive fixture for the amended repeated-value rule (REQ-001): maximal-
# shape subsumption and per-scope reporting.
#
# Rule 1: the duplicated shape S = distance(arr[I], arr[I]) + distance(arr[I], arr[I])
# contains the duplicated inner shape d = distance(arr[I], arr[I]); nested
# duplicates are subsumed, so only the maximal shape S fires (1 finding).
#
# Rule 2: the same duplicated shape S in two separate loop scopes of one rule
# fires once per scope (2 findings).
#
# Wright-authored fixture (AGPL-3.0-or-later); no third-party material.
#
# Expected: exactly three `repeated-value` findings, each stating "evaluated
# 2 times within the same loop scope".

globalvar I = 0
globalvar arr = [vect(0, 0, 0), vect(1, 0, 0), vect(2, 0, 0)]

rule "maximal shape subsumes nested duplicate":
    @Event global
    for I in range(3):
        debug(distance(arr[I], arr[I]) + distance(arr[I], arr[I]))
        debug(distance(arr[I], arr[I]) + distance(arr[I], arr[I]))

rule "same shape in two loop scopes":
    @Event global
    for I in range(3):
        debug(distance(arr[I], arr[I]) + distance(arr[I], arr[I]))
        debug(distance(arr[I], arr[I]) + distance(arr[I], arr[I]))
    for I in range(2):
        debug(distance(arr[I], arr[I]) + distance(arr[I], arr[I]))
        debug(distance(arr[I], arr[I]) + distance(arr[I], arr[I]))
