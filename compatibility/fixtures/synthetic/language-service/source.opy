globalvar score = 0

subroutine showStatus

def showStatus():
    print("Score: {}".format(score))

rule "global":
    @Event global
    showStatus()
