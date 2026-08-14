# Positive fixture for the amended repeated-value rule (REQ-001), santa-style:
# a `For Global Variable` loop whose body evaluates the identical
# `vectorTowards(...)` sub-expression across sibling `Modify Global Variable`
# (Append To Array) actions, with corpus-realistic inner calls (mirror of
# overpy-santa workshop lines 108-112 / `santa.opy:72-77`, where each
# `Vector Towards` family contains `Value In Array` and `Multiply` reads).
#
# AB = vectorTowards(rectangleChimneys[i*4]   * vect(1,0,1),
#                    rectangleChimneys[i*4+1] * vect(1,0,1))   -> 3 occurrences
# AD = vectorTowards(rectangleChimneys[i*4+2] * vect(1,0,1),
#                    rectangleChimneys[i*4+3] * vect(1,0,1))   -> 3 occurrences
#
# Under the per-shape reporting rule (REQ-001) each family fires exactly one
# finding, at its first occurrence, with the occurrence count in the message
# (3 each): 2 findings total. Inner duplicated shapes (e.g. the per-index
# `Value In Array`/`Multiply` reads) are subsumed by the maximal
# `vectorTowards` shape; bare single-call reads are never flagged.
#
# Wright-authored fixture (AGPL-3.0-or-later); no third-party material.
#
# Expected: exactly two `repeated-value` findings (AB family, AD family),
# each stating "evaluated 3 times within the same loop scope".

globalvar i = 0
globalvar rectangleChimneys = [vect(0, 0, 0), vect(1, 0, 0), vect(2, 0, 0), vect(3, 0, 0)]
globalvar rectangleChimneysAB = []
globalvar rectangleChimneysAD = []

rule "repeated vector towards across sibling actions":
    @Event global
    for i in range(4):
        rectangleChimneysAB.append(vectorTowards(rectangleChimneys[i * 4] * vect(1, 0, 1), rectangleChimneys[i * 4 + 1] * vect(1, 0, 1)))
        rectangleChimneysAD.append(vectorTowards(rectangleChimneys[i * 4 + 2] * vect(1, 0, 1), rectangleChimneys[i * 4 + 3] * vect(1, 0, 1)))
        rectangleChimneysAB.append(dotProduct(vectorTowards(rectangleChimneys[i * 4] * vect(1, 0, 1), rectangleChimneys[i * 4 + 1] * vect(1, 0, 1)), vectorTowards(rectangleChimneys[i * 4] * vect(1, 0, 1), rectangleChimneys[i * 4 + 1] * vect(1, 0, 1))))
        rectangleChimneysAD.append(dotProduct(vectorTowards(rectangleChimneys[i * 4 + 2] * vect(1, 0, 1), rectangleChimneys[i * 4 + 3] * vect(1, 0, 1)), vectorTowards(rectangleChimneys[i * 4 + 2] * vect(1, 0, 1), rectangleChimneys[i * 4 + 3] * vect(1, 0, 1))))
