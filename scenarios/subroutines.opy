globalvar score = 0
subroutine showStatus

def showStatus():
    print("score: {}".format(score))

rule "start":
    @Event global
    showStatus()
