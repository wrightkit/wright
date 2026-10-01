settings {
    "main": {"description": "Score race"},
    "gamemodes": {"ffa": {"enabledMaps": ["workshopIsland"]}}
}

playervar score

rule "credit killer":
    @Event playerEarnedElimination
    attacker.score += 1

rule "show score":
    @Event eachPlayer
    hudHeader(eventPlayer, "Score: {0}".format(eventPlayer.score), HudPosition.LEFT, 0, Color.WHITE, HudReeval.VISIBILITY_AND_STRING)

rule "declare winner":
    @Event eachPlayer
    @Condition eventPlayer.score >= 10
    declarePlayerVictory(eventPlayer)
