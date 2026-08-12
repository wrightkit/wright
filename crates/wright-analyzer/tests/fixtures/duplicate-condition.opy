globalvar index = 0

rule "duplicate conditions":
    @Event global
    if index == 0:
        debug(1)
    elif index == 0:
        debug(2)
