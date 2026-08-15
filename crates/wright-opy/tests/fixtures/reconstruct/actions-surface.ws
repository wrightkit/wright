variables {
    global:
        0: position
        1: score
}

subroutines {
    0: apply
}

rule ("Subroutine apply") {
    event {
        Subroutine;
        apply;
    }
    actions {
        Set Global Variable(position, 1);
    }
}

rule ("actions surface") {
    event {
        Ongoing - Global;
    }
    actions {
        Disable Inspector Recording;
        Wait(0.016, Ignore Condition);
        Wait(1, Ignore Condition);
        Play Effect(Event Player, Bad Explosion, Yellow, Global.position, 3);
        Chase Global Variable Over Time(Global.score, 10, 3, Destination and Duration);
        Call Subroutine(apply);
    }
}
