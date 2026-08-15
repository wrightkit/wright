variables {
    global:
        0: score
        1: points
        8: I
    player:
        0: hasStarted
        1: kills
}

subroutines {
    0: showStatus
    1: resetScore
}

rule ("Initialize global variables") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(score, 5);
        Set Global Variable(points, 0.0);
    }
}

rule ("Initialize player variables") {
    event {
        Ongoing - Each Player;
    }
    actions {
        Set Player Variable(Event Player, kills, 3);
    }
}

rule ("main") {
    event {
        Ongoing - Global;
    }
    actions {
        Call Subroutine(resetScore);
        Set Player Variable(Event Player, hasStarted, True);
        Set Global Variable(I, 10);
        Set Global Variable(score, Compare(Global.score, ==, 5));
    }
}
