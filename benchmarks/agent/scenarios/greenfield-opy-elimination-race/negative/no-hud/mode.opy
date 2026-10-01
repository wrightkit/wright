settings {
    "main": {"description": "First to seven"},
    "gamemodes": {"ffa": {"enabledMaps": ["workshopIsland"]}}
}

globalvar targetScore
playervar score

rule "configure match":
    targetScore = 7

rule "force soldier":
    @Event eachPlayer
    eventPlayer.startForcingHero(Hero.SOLDIER)

rule "score on elimination":
    @Event playerEarnedElimination
    @Condition attacker != victim
    attacker.score += 1

rule "declare winner":
    @Event eachPlayer
    @Condition eventPlayer.score >= targetScore
    declarePlayerVictory(eventPlayer)
