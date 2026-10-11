playervar other

rule "Proximity repel":
    @Event eachPlayer
    @Condition distance(eventPlayer, eventPlayer.other) <= 6

    eventPlayer.applyImpulse(directionTowards(eventPlayer.other, eventPlayer), 150, Relativity.TO_WORLD, Impulse.CANCEL_CONTRARY_MOTION_XYZ)
    eventPlayer.other.applyImpulse(directionTowards(eventPlayer, eventPlayer.other), 150, Relativity.TO_WORLD, Impulse.CANCEL_CONTRARY_MOTION_XYZ)
    eventPlayer.setStatusEffect(null, Status.STUNNED, 0.75)
    eventPlayer.other.setStatusEffect(null, Status.STUNNED, 0.75)
