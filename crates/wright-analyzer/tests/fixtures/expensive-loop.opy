globalvar pos = vect(0, 0, 0)

rule "expensive check in loop":
    @Event global
    while true:
        if distance(pos, vect(1, 2, 3)) < 5:
            debug(1)
        wait(0.1)
