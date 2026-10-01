settings {
    "main": {"description": "First to seven"},
    "gamemodes": {"ffa": {"enabledMaps": ["workshopIsland"]}}
}

globalvar targetScore
playervar score

rule "configure match":
    targetScore = 7
    hudHeader(getAllPlayers(), "First to {0}".format(targetScore), HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY_AND_STRING)

rule "force soldier":
    @Event eachPlayer
    eventPlayer.startForcingHero(Hero.SOLDIER)

rule "score on elimination":
    @Event playerEarnedElimination
    attacker.score += 1

rule "declare winner":
    @Event eachPlayer
    @Condition eventPlayer.score >= targetScore
    declarePlayerVictory(eventPlayer)
