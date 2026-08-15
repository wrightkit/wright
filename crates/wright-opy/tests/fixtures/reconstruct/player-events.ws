variables {
    global:
        0: lastKiller
    player:
        0: hasStarted
}

subroutines {
    0: onDeath
}

rule ("Subroutine onDeath") {
    event {
        Subroutine;
        onDeath;
    }
    actions {
        Set Global Variable(lastKiller, Position Of(Event Player));
    }
}

rule ("player starts") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Compare((Event Player).hasStarted, ==, True);
        Compare(Is Alive(Event Player), ==, True);
    }
    actions {
        Set Player Variable(Event Player, hasStarted, True);
        Set Move Speed(Event Player, 100);
        Set Max Health(Event Player, 50);
        Set Invisible(Event Player, All);
        Set Status(Event Player, Null, Asleep, 1.5);
        Teleport(Event Player, Global.lastKiller);
        Call Subroutine(onDeath);
    }
}
