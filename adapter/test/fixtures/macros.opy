macro double(value):
    value + value

rule "macro use":
    @Event global
    debug(double(2))
