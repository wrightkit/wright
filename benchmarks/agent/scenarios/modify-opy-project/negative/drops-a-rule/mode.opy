settings {
    "main": {"description": "Streak race"},
    "gamemodes": {"ffa": {"enabledMaps": ["workshopIsland"]}}
}

#!define TARGET_SCORE 7

globalvar matchOver
playervar score
playervar streak

rule "reset streak on spawn":
    @Event eachPlayer
    @Condition eventPlayer.hasSpawned()
    eventPlayer.streak = 0

rule "score on elimination":
    @Event playerEarnedElimination
    attacker.score += 1
    attacker.streak += 1

rule "declare winner":
    @Event eachPlayer
    @Condition eventPlayer.score >= TARGET_SCORE
    @Condition matchOver == false
    matchOver = true
    declarePlayerVictory(eventPlayer)

rule "target display":
    @Event eachPlayer
    hudHeader(getAllPlayers(), "First to {0}".format(TARGET_SCORE), HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY)

rule "score display":
    @Event eachPlayer
    hudHeader(eventPlayer, "Score: {0}".format(eventPlayer.score), HudPosition.LEFT, 0, Color.WHITE, HudReeval.VISIBILITY_AND_STRING)
