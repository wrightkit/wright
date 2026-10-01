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
    heal(eventPlayer, null, 50)

rule "reward on elimination":
    @Event playerEarnedElimination
    grantReward()

rule "reward on assist":
    @Event playerDealtFinalBlow
    attacker.streak += 1
    attacker.setUltCharge(100)
    bigMessage(attacker, "Streak {0}".format(attacker.streak))
    heal(attacker, null, 50)

rule "reset streak":
    @Event playerDied
    eventPlayer.streak = 0
