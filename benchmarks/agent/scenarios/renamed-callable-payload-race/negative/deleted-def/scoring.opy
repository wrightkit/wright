# Scoring rules share the helpers from helpers.opy.

rule "overtime nudge":
    @Event eachPlayer
    @Condition eventPlayer.tags >= 2

rule "reset on death":
    @Event playerDied
    clearTags()
