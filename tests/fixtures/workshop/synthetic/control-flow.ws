variables {
    global:
        0: index
}

rule ("bounded while") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.index, <, 3));
            Modify Global Variable(index, Add, 1);
            Wait(0.016, Ignore Condition);
        End;
    }
}

rule ("branch") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.index, ==, 0));
            Set Global Variable(index, 1);
        End;
    }
}
