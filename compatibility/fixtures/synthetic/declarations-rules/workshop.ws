variables {
    global:
        0: score
    player:
        0: hasStarted
}

subroutines {
    0: showStatus
}

rule ("Subroutine showStatus") {
    event {
        Subroutine;
        showStatus;
    }
    actions {
        Set Global Variable(score, 1);
    }
}

rule ("player starts") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Has Spawned(Event Player) == True;
    }
    actions {
        Set Player Variable(Event Player, hasStarted, True);
        Call Subroutine(showStatus);
    }
}
