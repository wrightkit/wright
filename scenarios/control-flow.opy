globalvar value = 5

rule "branching":
    @Event global
    if value == 0:
        debug(value)
    elif value < 10:
        debug(1)
    else:
        debug(2)
