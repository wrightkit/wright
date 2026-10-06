#!define TAG_LIMIT 3

settings {
    "main": {"description": "Payload race"},
    "gamemodes": {"ffa": {"enabledMaps": ["workshopIsland"]}}
}

globalvar round
playervar tags

#!include "helpers.opy"
#!include "scoring.opy"

rule "tag carrier":
    @Event playerEarnedElimination
    eventPlayer.tags += 1

rule "round ticks":
    @Event eachPlayer
    @Condition round < TAG_LIMIT

rule "declare winner":
    @Event eachPlayer
    @Condition round >= TAG_LIMIT
    declarePlayerVictory(eventPlayer)
