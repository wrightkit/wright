settings {
    "main": {"description": "Reward demo"},
    "gamemodes": {"ffa": {"enabledMaps": ["workshopIsland"]}}
}

playervar streak

def grantReward():
    @Name "grantReward"
    eventPlayer.streak += 1
    eventPlayer.setUltCharge(100)
    bigMessage(eventPlayer, "Streak {0}".format(eventPlayer.streak))

rule "reward on elimination":
    @Event playerEarnedElimination
    grantReward()

rule "reward on assist":
    @Event playerDealtFinalBlow
    grantReward()

rule "reset streak":
    @Event playerDied
    eventPlayer.streak = 0
