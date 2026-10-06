# Scoring helpers shared by the rules in mode.opy.

def awardBonus_v2():
    @Name "awardBonus"
    eventPlayer.score += 2
    eventPlayer.streak = 0
