settings {
    "main": {"description": "First to five"},
    "gamemodes": {"ffa": {"enabledMaps": ["workshopIsland"]}}
}

playervar score

rule "score on elimination":
    @Event playerEarnedElimination
    attacker.score += 1

rule "declare winner":
    @Event eachPlayer
    @Condition eventPlayer.score >= 5
    declarePlayerVictory(eventPlayer)
