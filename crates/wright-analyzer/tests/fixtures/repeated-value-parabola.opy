# Positive fixture for the amended repeated-value rule (REQ-001),
# parabola-style: one action inside a `For Global Variable` loop whose value
# surface contains two duplicated shape families with corpus-realistic inner
# calls (mirror of overpy-parabola workshop lines 79-80 /
# `parabola.opy:45-51`, where an identical `Distance Between(...)` appears
# twice and an identical `Subtract(...)` appears six times within one
# `Create In-World Text` action):
#
#   distance(eventPlayer.getEyePosition(), target[I])   -> 2 occurrences
#   time - offsets[I]                                   -> 3 occurrences
#
# Under the per-shape reporting rule (REQ-001) each family fires exactly one
# finding, at its first occurrence, with the occurrence count in the message
# (2 and 3): 2 findings total. Inner single-call reads (`target[I]`,
# `offsets[I]`, `getEyePosition(...)`) are never flagged.
#
# Wright-authored fixture (AGPL-3.0-or-later); no third-party material.
#
# Expected: exactly two `repeated-value` findings — "evaluated 2 times" for
# the distance family, "evaluated 3 times" for the subtract family.

globalvar I = 0
globalvar target = [vect(1, 0, 0), vect(0, 1, 0), vect(0, 0, 1)]
globalvar time = 10
globalvar offsets = [1, 2, 3]

rule "repeated shapes within one action":
    @Event global
    for I in range(3):
        debug(distance(eventPlayer.getEyePosition(), target[I]) + distance(eventPlayer.getEyePosition(), target[I]) + (time - offsets[I]) + (time - offsets[I]) + (time - offsets[I]))
