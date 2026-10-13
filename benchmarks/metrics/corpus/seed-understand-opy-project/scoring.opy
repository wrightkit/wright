# Scoring helpers shared by the rules in mode.opy.

def awardBonus():
    @Name "awardBonus"
    eventPlayer.score += 2
    eventPlayer.streak = 0
