variables {
    global:
        0: result
        1: center
    player:
        0: status
}

rule ("expressions and values") {
    event {
        Ongoing - Global;
    }
    conditions {
        Compare(Global.result, >=, 0);
        Compare(And(Compare(Global.result, >, 0), Not(Compare(Global.result, <, 5))), ==, True);
        Compare(Is Game In Progress(), ==, True);
    }
    actions {
        Set Global Variable(result, 42);
        Set Global Variable(result, "hello");
        Set Global Variable(result, False);
        Set Global Variable(result, Null);
        Set Global Variable(result, Players Within Radius(Global.center, 5, All Teams, Off));
        Set Global Variable(result, World Vector Of(Global.center, Event Player, Rotation));
        Set Global Variable(result, Health(Event Player));
        Set Global Variable(result, Throttle Of(Event Player));
        Set Player Variable(Event Player, status, 1);
        Set Global Variable(result, Global.result);
    }
}
