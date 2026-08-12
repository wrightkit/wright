globalvar items = [1, 2, 3]
globalvar first

rule "array ops":
    @Event global
    items.append(4)
    first = items[0]
