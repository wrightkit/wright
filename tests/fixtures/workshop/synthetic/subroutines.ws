variables {
    global:
        0: score
}

subroutines {
    0: shared
    1: helper
}

rule ("Subroutine shared") {
    event {
        Subroutine;
        shared;
    }
    actions {
        Set Global Variable(score, 1);
    }
}

rule ("Subroutine helper") {
    event {
        Subroutine;
        helper;
    }
    actions {
        Set Global Variable(score, 2);
    }
}

rule ("first") {
    event {
        Ongoing - Global;
    }
    actions {
        Call Subroutine(shared);
        Call Subroutine(helper);
    }
}

rule ("second") {
    event {
        Ongoing - Global;
    }
    actions {
        Call Subroutine(shared);
    }
}
