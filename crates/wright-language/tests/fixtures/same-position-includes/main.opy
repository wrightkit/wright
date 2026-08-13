#!include "a.opy"

globalvar score = 0

rule "main":
    @Event global
    score += 1
