settings {
    "main": {
        "modeName": "Ana Paintball",
        "description": "Every Ana rifle shot kills. Scoped shots pierce. Sleep darts + melee kill (and cost the victim a point). 5/10/15 streaks, nuke at 15, dart-only finish at 24. First to 25."
    },
    "gamemodes": {
        "ffa": {
            "enabledMaps": ["blackForest", "chateauGuillard", "eichenwalde", "kanezaka"],
            "scoreToWin": 25
        }
    }
}

#Tunables
#!define SCORE_TO_WIN 25
#!define DART_ONLY_SCORE 24
#!define DART_COOLDOWN 2.5
#!define NUKE_STREAK 15
#!define PIERCE_RANGE 200
#!define PIERCE_RADIUS 0.6

playervar streak
playervar nukeActive
playervar nukeFired
globalvar pierceEye
globalvar pierceDir
globalvar pierceWall
globalvar pierceCandidates
globalvar pierceTarget
globalvar pierceIdx
globalvar pierceAlong
globalvar pierceOnRay

# ---------------------------------------------------------------------
# Hero lock: everybody is Ana
# ---------------------------------------------------------------------
rule "Force Ana":
    @Event eachPlayer
    @Condition eventPlayer.hasSpawned()
    @Condition not eventPlayer.nukeActive

    eventPlayer.startForcingHero(Hero.ANA)
    eventPlayer.nukeFired = eventPlayer.isButtonHeld(Button.PRIMARY_FIRE)

# ---------------------------------------------------------------------
# Every rifle shot kills instantly; scoped shots pierce enemies.
# The sleep dart is ability 1, so it is never handled here.
# ---------------------------------------------------------------------
rule "Rifle hit kills":
    @Event playerDealtDamage
    @Condition eventAbility == Button.PRIMARY_FIRE
    @Condition attacker != victim
    @Condition attacker.nukeActive == false
    @Condition attacker.getScore() < DART_ONLY_SCORE

    kill(victim, attacker)

    # Scoped: also kill every other enemy on the line of fire up to the first wall.
    # No waits in here, so the shared scratch globals cannot interleave between shots.
    if attacker.isFiringSecondaryFire():
        pierceEye = attacker.getEyePosition()
        pierceDir = attacker.getFacingDirection()
        pierceWall = distance(pierceEye, raycast(pierceEye, pierceEye + pierceDir * PIERCE_RANGE, [], [attacker], false).getHitPosition())
        pierceCandidates = getLivingPlayers(Team.ALL)
        for pierceIdx in range(len(pierceCandidates)):
            pierceTarget = pierceCandidates[pierceIdx]
            if pierceTarget == attacker:
                continue
            pierceAlong = dotProduct(pierceTarget.getPosition() + Vector.UP * 0.9 - pierceEye, pierceDir)
            if pierceAlong < 0 or pierceAlong > pierceWall:
                continue
            pierceOnRay = pierceEye + pierceDir * pierceAlong
            if min(min(distance(pierceTarget.getPosition() + Vector.UP * 0.3, pierceOnRay),
                       distance(pierceTarget.getPosition() + Vector.UP * 0.9, pierceOnRay)),
                   distance(pierceTarget.getEyePosition(), pierceOnRay)) <= PIERCE_RADIUS:
                kill(pierceTarget, attacker)

# ---------------------------------------------------------------------
# Melee on a sleeping enemy: kill, and the victim loses a point.
# ---------------------------------------------------------------------
rule "Melee kills sleeper":
    @Event playerDealtDamage
    @Condition eventAbility == Button.MELEE
    @Condition attacker != victim
    @Condition victim.hasStatus(Status.ASLEEP)

    kill(victim, attacker)
    victim.addToScore(-1)

# ---------------------------------------------------------------------
# Streaks and rampage messages
# ---------------------------------------------------------------------
rule "Death bookkeeping":
    @Event playerDied

    eventPlayer.streak = 0
    if eventPlayer.nukeActive:
        eventPlayer.nukeActive = false
        eventPlayer.nukeFired = false

rule "Count kill":
    @Event playerDied
    @Condition attacker != null
    @Condition attacker != eventPlayer

    attacker.streak += 1
    if attacker.streak == 5:
        bigMessage(getAllPlayers(), "{0} is on a RAMPAGE! 5 kills in a row".format(attacker))
    elif attacker.streak == 10:
        bigMessage(getAllPlayers(), "{0} is UNSTOPPABLE! 10 kills in a row".format(attacker))
    elif attacker.streak == NUKE_STREAK:
        bigMessage(getAllPlayers(), "{0} is GODLIKE! 15 kills in a row - NUCLEAR STRIKE incoming".format(attacker))

# ---------------------------------------------------------------------
# Nuclear strike: Doomfist's Meteor Strike, granted at 15 in a row.
# Killing anyone with it wins the game instantly.
# ---------------------------------------------------------------------
rule "Grant nuke":
    @Event eachPlayer
    @Condition eventPlayer.streak == NUKE_STREAK
    @Condition eventPlayer.nukeActive == false
    @Condition eventPlayer.isAlive()

    eventPlayer.nukeActive = true
    eventPlayer.nukeFired = false
    eventPlayer.startForcingHero(Hero.DOOMFIST)
    wait(0.25)
    eventPlayer.setPrimaryFireEnabled(false)
    eventPlayer.setSecondaryFireEnabled(false)
    eventPlayer.setAbility1Enabled(false)
    eventPlayer.setAbility2Enabled(false)
    eventPlayer.setMeleeEnabled(false)
    eventPlayer.setUltCharge(100)
    bigMessage(eventPlayer, "NUCLEAR STRIKE READY - use your ultimate!")

rule "Nuke fired":
    @Event eachPlayer
    @Condition eventPlayer.nukeActive
    @Condition eventPlayer.isUsingUltimate()

    eventPlayer.nukeFired = true

rule "Nuke kill wins":
    @Event playerDied
    @Condition attacker != null
    @Condition attacker != eventPlayer
    @Condition attacker.nukeActive
    @Condition (attacker.nukeFired or eventAbility == Button.ULTIMATE)

    declarePlayerVictory(attacker)

rule "Nuke spent":
    @Event eachPlayer
    @Condition eventPlayer.nukeActive
    @Condition eventPlayer.nukeFired
    @Condition not eventPlayer.isUsingUltimate()

    # Let late meteor kills register before handing the rifle back.
    wait(1.5, Wait.ABORT_WHEN_FALSE)
    eventPlayer.nukeActive = false
    eventPlayer.nukeFired = false
    eventPlayer.startForcingHero(Hero.ANA)
    wait(0.25)
    eventPlayer.setPrimaryFireEnabled(true)
    eventPlayer.setSecondaryFireEnabled(true)
    eventPlayer.setAbility1Enabled(true)
    eventPlayer.setAbility2Enabled(true)
    eventPlayer.setMeleeEnabled(true)

# ---------------------------------------------------------------------
# Score 24: rifle off, dart cooldown 2.5s
# ---------------------------------------------------------------------
rule "Dart-only at 24":
    @Event eachPlayer
    @Condition eventPlayer.hasSpawned()

    while true:
        eventPlayer.setPrimaryFireEnabled(eventPlayer.getScore() < DART_ONLY_SCORE and not eventPlayer.nukeActive)
        wait(0.25, Wait.IGNORE_CONDITION)

rule "Fast dart at 24":
    @Event eachPlayer
    @Condition eventPlayer.getScore() >= DART_ONLY_SCORE
    @Condition eventPlayer.getAbilityCooldown(Button.ABILITY_1) > DART_COOLDOWN

    eventPlayer.setAbilityCooldown(Button.ABILITY_1, DART_COOLDOWN)

# ---------------------------------------------------------------------
# HUD
# ---------------------------------------------------------------------
rule "HUD":
    @Event eachPlayer

    hudHeader(eventPlayer, "Streak {0}".format(eventPlayer.streak), HudPosition.LEFT, 0, Color.WHITE, HudReeval.VISIBILITY_AND_STRING)
    hudSubheader(eventPlayer if eventPlayer.getScore() >= DART_ONLY_SCORE else null, "Rifle OFF - land a sleep dart, then melee", HudPosition.LEFT, 1, Color.RED, HudReeval.VISIBILITY_AND_STRING)
