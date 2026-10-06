#!define TARGET_SCORE 5

settings {
    "main": {"description": "Score race"},
    "gamemodes": {"ffa": {"enabledMaps": ["workshopIsland"]}}
}

globalvar unusedBanner
playervar score
playervar streak
playervar bonus

# TODO: bonus points are handled by a subroutine for now.
#!include "scoring.opy"

rule "score on elimination":
    @Event playerEarnedElimination
    attacker.score += 1
    attacker.streak += 1

rule "streak bonus points":
    @Event eachPlayer
    @Condition eventPlayer.streak >= 3

rule "reset streak on death":
    @Event playerDied
    eventPlayer.streak = 0

rule "declare winner":
    @Event eachPlayer
    @Condition eventPlayer.score >= TARGET_SCORE
    declarePlayerVictory(eventPlayer)

rule "score display":
    @Event eachPlayer
    hudHeader(eventPlayer, "Score: {0}".format(eventPlayer.score), HudPosition.LEFT, 0, Color.WHITE, HudReeval.VISIBILITY_AND_STRING)
