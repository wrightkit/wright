settings {
    "main": {"description": "Countdown demo"},
    "gamemodes": {"ffa": {"enabledMaps": ["workshopIsland"]}}
}

globalvar countdown
globalvar ticks

rule "start countdown":
    countdown = 10

rule "tick counter":
    pass

rule "run countdown":
    while countdown > 0:
        wait(1)
        countdown -= 1
