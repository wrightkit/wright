globalvar points = [1, 2, 3]
globalvar count = 0

rule "counters":
    @Event global
    count += 1
    count = count * 2
