variables {
    global:
        0: countdown
        1: ticks
}

rule ("start countdown") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(countdown, 10);
    }
}

rule ("tick counter") {
    event {
        Ongoing - Global;
    }
    actions {
        While(True);
            Modify Global Variable(ticks, Add, 1);
            Wait(1, Ignore Condition);
        End;
    }
}

rule ("run countdown") {
    event {
        Ongoing - Global;
    }
    conditions {
        Global.countdown > 0;
    }
    actions {
        Wait(1, Ignore Condition);
        Modify Global Variable(countdown, Subtract, 1);
        Loop If Condition Is True;
    }
}
