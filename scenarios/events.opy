playervar hasStarted

rule "global event":
    @Event global
    debug("hello")

rule "player event":
    @Event eachPlayer
    @Condition eventPlayer.hasSpawned()
    eventPlayer.hasStarted = true
