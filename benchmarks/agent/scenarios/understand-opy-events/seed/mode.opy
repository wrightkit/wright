settings {
    "main": {"description": "Round demo"},
    "gamemodes": {"ffa": {"enabledMaps": ["workshopIsland"]}}
}

globalvar matchStarted
globalvar roundTimer
globalvar leader
globalvar spareFlag
globalvar bonusPoints

playervar kills
playervar deaths
playervar shield

# spareFlag was used by an older timer; nothing uses it now.

def awardPoint():
    @Name "awardPoint"
    eventPlayer.kills += 1
    bonusPoints += 1

def refreshLeader():
    @Name "refreshLeader"
    leader = sorted(getAllPlayers(), lambda p: -p.kills)[0]

def resetTimer():
    @Name "resetTimer"
    roundTimer = 300

rule "start match":
    matchStarted = true
    resetTimer()

rule "countdown":
    @Condition matchStarted == true
    while roundTimer > 0:
        wait(1)
        roundTimer -= 1

rule "restart on empty":
    @Condition len(getAllPlayers()) == 0
    resetTimer()

rule "kill credit":
    @Event playerEarnedElimination
    awardPoint()
    refreshLeader()

rule "death count":
    @Event playerDied
    eventPlayer.deaths += 1

rule "leader hud":
    @Event eachPlayer
    hudHeader(eventPlayer, "Leader: {0}".format(leader), HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY_AND_STRING)

rule "spawn shield":
    @Event eachPlayer
    @Condition eventPlayer.hasSpawned()
    eventPlayer.shield = 100
