globalvar alpha = 0

rule "r":
    @Event global
    debug(alpha)

    alpha = alpha + 1
