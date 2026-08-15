variables {
    global:
        0: index
        1: points
    player:
        0: hasStarted
}

subroutines {
    0: tick
}

rule ("Subroutine tick") {
    event {
        Subroutine;
        tick;
    }
    actions {
        Modify Global Variable(index, Add, 1);
        Set Player Variable(Event Player, hasStarted, True);
    }
}

rule ("control flow") {
    event {
        Ongoing - Global;
    }
    actions {
        Modify Global Variable(points, Append To Array, Global.index);
        Modify Global Variable(index, Raise To Power, 2);
        For Global Variable(index, 0, 3, 1);
            If(Compare(Global.index, ==, 0));
                Call Subroutine(tick);
            Else If(Compare(Global.index, ==, 1));
                Set Global Variable(index, 5);
            Else;
                Modify Player Variable(Event Player, hasStarted, Subtract, 1);
            End;
        End;
        While(Compare(Global.index, <, 3));
            Modify Global Variable(index, Add, 1);
            Wait(0.016, Ignore Condition);
        End;
    }
}
