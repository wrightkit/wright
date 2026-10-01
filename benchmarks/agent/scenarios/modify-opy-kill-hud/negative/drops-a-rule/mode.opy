settings {
    "main": {"description": "Kill race"},
    "gamemodes": {"ffa": {"enabledMaps": ["workshopIsland"]}}
}

#!define TARGET_KILLS 5

playervar kills

rule "force heroes":
    @Event eachPlayer
    @Condition eventPlayer.hasSpawned()
    eventPlayer.startForcingHero(Hero.ANA)

rule "count kill":
    @Event playerEarnedElimination
    @Condition attacker != victim
    attacker.kills += 1

rule "kills display":
    @Event eachPlayer
    hudHeader(eventPlayer, "Eliminations: {0}".format(eventPlayer.kills), HudPosition.LEFT, 0, Color.WHITE, HudReeval.VISIBILITY_AND_STRING)

rule "declare winner":
    @Event eachPlayer
    @Condition eventPlayer.kills >= TARGET_KILLS
    declarePlayerVictory(eventPlayer)
