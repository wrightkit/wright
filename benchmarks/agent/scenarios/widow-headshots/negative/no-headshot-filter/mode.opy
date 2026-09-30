settings {
    "main": {
        "modeName": "Widowmaker Headshots",
        "description": "Headshots only. 10 kills in a row earns Infra-Sight."
    },
    "gamemodes": {
        "ffa": {
            "enabledMaps": ["necropolis", "chateauGuillard", "castillo"],
            "scoreToWin": 200
        }
    },
    "heroes": {
        "allTeams": {
            "enabledHeroes": ["widowmaker"],
            "widowmaker": {
                "passiveHealthRegen": false
            }
        }
    }
}

# Workshop settings
# HP choice: 0 = Default, 1 = 175, 2 = 200, 3 = 225 (this OverPy frontend rejects enum settings)
#!define hpChoice createWorkshopSetting(int[0:3], "Widowmaker", "Widowmaker HP (0=Default 1=175 2=200 3=225)", 0, 0)
#!define removeFalloff createWorkshopSetting(bool, "Widowmaker", "Remove damage falloff", true, 1)
#!define killsToWin createWorkshopSetting(int[1:200], "Match", "Kills to win", 50, 0)

# Tuning
#!define GRAPPLE_COOLDOWN 2.5
#!define INFRA_SIGHT_STREAK 10
#!define INFRA_SIGHT_SECONDS 10
#!define KILL_HEAL_FRACTION 0.25
#!define UNSCOPED_HEADSHOT_DAMAGE 26

globalvar globalShots
globalvar globalHeadshots

playervar kills
playervar streak
playervar shots
playervar headshots
playervar joinTime
playervar hpCap
playervar lastAmmo
playervar ignoreNextHit
playervar infraReady
playervar statsOn
playervar killfeedOn
playervar bigHud

rule "Init globals":
    globalShots = 0
    globalHeadshots = 0

rule "Init player":
    @Event eachPlayer
    eventPlayer.startForcingHero(Hero.WIDOWMAKER)
    eventPlayer.kills = 0
    eventPlayer.streak = 0
    eventPlayer.shots = 0
    eventPlayer.headshots = 0
    eventPlayer.ignoreNextHit = false
    eventPlayer.infraReady = false
    eventPlayer.statsOn = true
    eventPlayer.killfeedOn = true
    eventPlayer.bigHud = false
    eventPlayer.joinTime = getTotalTimeElapsed()

rule "Spawn setup":
    @Event eachPlayer
    @Condition eventPlayer.isAlive() == true
    eventPlayer.hpCap = 9999
    eventPlayer.setMaxHealth(100)
    wait(0.1)
    if hpChoice == 1:
        eventPlayer.setMaxHealth(175 / eventPlayer.getMaxHealth() * 100)
    elif hpChoice == 2:
        eventPlayer.setMaxHealth(200 / eventPlayer.getMaxHealth() * 100)
    elif hpChoice == 3:
        eventPlayer.setMaxHealth(225 / eventPlayer.getMaxHealth() * 100)
    wait(0.1)
    heal(eventPlayer, null, 9999)
    wait(0.1)
    eventPlayer.hpCap = eventPlayer.getHealth()
    eventPlayer.lastAmmo = eventPlayer.getAmmo(0)

rule "Grappling Hook cooldown":
    @Event eachPlayer
    @Condition eventPlayer.getAbilityCooldown(Button.ABILITY_1) > GRAPPLE_COOLDOWN
    eventPlayer.setAbilityCooldown(Button.ABILITY_1, GRAPPLE_COOLDOWN)

# Only headshots do damage: a body hit is refunded the moment it lands.
rule "Refund non-headshot damage":
    @Event playerTookDamage
    if not eventPlayer.isAlive():
        return
    if eventPlayer.ignoreNextHit:
        eventPlayer.ignoreNextHit = false
        eventPlayer.hpCap = eventPlayer.getHealth()
        return
    if true:
        eventPlayer.hpCap = eventPlayer.getHealth()
        return
    eventPlayer.hpCap = eventPlayer.getHealth() + eventDamage
    heal(eventPlayer, null, eventDamage)

# No healthpacks: health may never rise above what damage and kill-heals allow.
rule "Block healthpacks":
    @Event eachPlayer
    @Condition eventPlayer.isAlive() == true
    @Condition eventPlayer.getHealth() > eventPlayer.hpCap + 1
    eventPlayer.ignoreNextHit = true
    damage(eventPlayer, null, eventPlayer.getHealth() - eventPlayer.hpCap)

rule "Remove damage falloff":
    @Event playerDealtDamage
    @Condition removeFalloff == true
    @Condition eventPlayer.isHoldingButton(Button.SECONDARY_FIRE) == false
    @Condition eventDamage < UNSCOPED_HEADSHOT_DAMAGE
    if not victim.isAlive():
        return
    victim.ignoreNextHit = true
    damage(victim, eventPlayer, UNSCOPED_HEADSHOT_DAMAGE - eventDamage)

rule "Count headshot hits":
    @Event playerDealtDamage
    eventPlayer.headshots += 1
    globalHeadshots += 1

rule "Count shots (ammo spent)":
    @Event eachPlayer
    @Condition eventPlayer.getAmmo(0) < eventPlayer.lastAmmo
    eventPlayer.shots += eventPlayer.lastAmmo - eventPlayer.getAmmo(0)
    globalShots += eventPlayer.lastAmmo - eventPlayer.getAmmo(0)
    eventPlayer.lastAmmo = eventPlayer.getAmmo(0)

rule "Track ammo refill":
    @Event eachPlayer
    @Condition eventPlayer.getAmmo(0) > eventPlayer.lastAmmo
    eventPlayer.lastAmmo = eventPlayer.getAmmo(0)

rule "Elimination":
    @Event playerEarnedElimination
    if victim == eventPlayer:
        return
    eventPlayer.kills += 1
    eventPlayer.streak += 1
    eventPlayer.hpCap = min(eventPlayer.getMaxHealth(), eventPlayer.hpCap + victim.getMaxHealth() * KILL_HEAL_FRACTION)
    heal(eventPlayer, null, victim.getMaxHealth() * KILL_HEAL_FRACTION)
    if eventPlayer.streak == INFRA_SIGHT_STREAK:
        eventPlayer.infraReady = true
        eventPlayer.setUltCharge(100)
        smallMessage(eventPlayer, "Infra-Sight ready!")
    if eventPlayer.kills >= killsToWin:
        declarePlayerVictory(eventPlayer)

rule "Death resets streak":
    @Event playerDied
    eventPlayer.streak = 0
    eventPlayer.infraReady = false
    eventPlayer.setUltCharge(0)

# Ultimate charge only ever comes from the killstreak.
rule "No natural ultimate charge":
    @Event eachPlayer
    @Condition eventPlayer.getUltCharge() > 0
    @Condition eventPlayer.infraReady == false
    eventPlayer.setUltCharge(0)

rule "Infra-Sight duration":
    @Event eachPlayer
    @Condition eventPlayer.isUsingUltimate() == true
    wait(INFRA_SIGHT_SECONDS, Wait.ABORT_WHEN_FALSE)
    eventPlayer.infraReady = false
    eventPlayer.setUltCharge(0)
    eventPlayer.setUltEnabled(false)
    wait(0.25)
    eventPlayer.setUltEnabled(true)

# Interact toggles stats, Crouch+Interact toggles killfeed, Melee+Interact toggles big/small HUD.
rule "HUD toggles":
    @Event eachPlayer
    @Condition eventPlayer.isHoldingButton(Button.INTERACT) == true
    if eventPlayer.isHoldingButton(Button.CROUCH):
        eventPlayer.killfeedOn = not eventPlayer.killfeedOn
        if eventPlayer.killfeedOn:
            eventPlayer.enableKillFeed()
        else:
            eventPlayer.disableKillFeed()
    elif eventPlayer.isHoldingButton(Button.MELEE):
        eventPlayer.bigHud = not eventPlayer.bigHud
    else:
        eventPlayer.statsOn = not eventPlayer.statsOn
    waitUntil(not eventPlayer.isHoldingButton(Button.INTERACT), 5)

# Personal HUD (small)
rule "HUD: personal stats":
    @Event eachPlayer
    hudHeader(eventPlayer, "Kills {0}/{1}  |  Streak {2}".format(eventPlayer.kills, killsToWin, eventPlayer.streak) if eventPlayer.statsOn else "", HudPosition.LEFT, 1, Color.WHITE, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)
    hudSubtext(eventPlayer, "Time {0}s  |  {1} kills/min".format(round(getTotalTimeElapsed() - eventPlayer.joinTime), round(eventPlayer.kills / max(1, (getTotalTimeElapsed() - eventPlayer.joinTime) / 60) * 10) / 10) if eventPlayer.statsOn else "", HudPosition.LEFT, 2, Color.SKY_BLUE, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)
    hudSubtext(eventPlayer, "Pace: {0}s to {1} kills".format(round(max(0, killsToWin - eventPlayer.kills) / max(0.01, eventPlayer.kills / max(1, (getTotalTimeElapsed() - eventPlayer.joinTime) / 60)) * 60), killsToWin) if eventPlayer.statsOn and eventPlayer.kills > 0 else "", HudPosition.LEFT, 3, Color.SKY_BLUE, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)
    hudSubtext(eventPlayer, "Your accuracy {0}%".format(round(eventPlayer.headshots / max(1, eventPlayer.shots) * 100)) if eventPlayer.statsOn else "", HudPosition.LEFT, 4, Color.SKY_BLUE, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)

# Big HUD adds global accuracy and killstreak leaders
rule "HUD: global stats":
    @Event eachPlayer
    hudHeader(eventPlayer, "Global accuracy {0}%".format(round(globalHeadshots / max(1, globalShots) * 100)) if eventPlayer.statsOn and eventPlayer.bigHud else "", HudPosition.LEFT, 5, Color.WHITE, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)
    hudSubtext(eventPlayer, "Streak leaders" if eventPlayer.statsOn and eventPlayer.bigHud else "", HudPosition.LEFT, 6, Color.ORANGE, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)
    hudSubtext(eventPlayer, "1. {0}: {1}".format(sorted(getAllPlayers(), lambda p: -p.streak)[0], sorted(getAllPlayers(), lambda p: -p.streak)[0].streak) if eventPlayer.statsOn and eventPlayer.bigHud and len(getAllPlayers()) >= 1 else "", HudPosition.LEFT, 7, Color.ORANGE, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)
    hudSubtext(eventPlayer, "2. {0}: {1}".format(sorted(getAllPlayers(), lambda p: -p.streak)[1], sorted(getAllPlayers(), lambda p: -p.streak)[1].streak) if eventPlayer.statsOn and eventPlayer.bigHud and len(getAllPlayers()) >= 2 else "", HudPosition.LEFT, 8, Color.ORANGE, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)
    hudSubtext(eventPlayer, "3. {0}: {1}".format(sorted(getAllPlayers(), lambda p: -p.streak)[2], sorted(getAllPlayers(), lambda p: -p.streak)[2].streak) if eventPlayer.statsOn and eventPlayer.bigHud and len(getAllPlayers()) >= 3 else "", HudPosition.LEFT, 9, Color.ORANGE, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)

rule "HUD: controls hint":
    @Event eachPlayer
    hudSubtext(eventPlayer, "Interact: stats | Crouch+Interact: killfeed | Melee+Interact: big HUD", HudPosition.RIGHT, 1, Color.GRAY, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)
