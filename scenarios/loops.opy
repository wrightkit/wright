globalvar i = 0

rule "bounded for":
    @Event global
    for i in range(3):
        debug(i)

rule "hot while":
    @Event global
    while true:
        wait()
