globalvar pos = vect(0, 0, 0)

rule "global expensive condition first":
    @Event global
    @Condition distance(pos, vect(1, 2, 3)) < 5
    @Condition isGameInProgress()
    debug(1)

rule "each player expensive condition":
    @Event eachPlayer
    @Condition isInLoS(eventPlayer.getPosition(), pos, BarrierLos.BLOCKED_BY_ALL_BARRIERS)
    debug(2)

rule "cheap ongoing condition":
    @Event global
    @Condition isGameInProgress()
    debug(3)
