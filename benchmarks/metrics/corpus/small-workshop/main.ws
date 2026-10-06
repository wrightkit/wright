variables {
    global:
        0: target_score
        1: round_active
    player:
        0: score
        1: combo
}

subroutines {
    0: award_point
}

rule ("configure match") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(target_score, 10);
        Set Global Variable(round_active, True);
    }
}

rule ("score on elimination") {
    event {
        Player Earned Elimination;
        All;
        All;
    }
    conditions {
        Global.round_active == True;
        Attacker != Victim;
    }
    actions {
        Call Subroutine(award_point);
        Modify Player Variable(Attacker, combo, Add, 1);
    }
}

rule ("combo bonus") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Event Player.combo >= 3;
    }
    actions {
        Modify Player Variable(Event Player, score, Add, 1);
        Set Player Variable(Event Player, combo, 0);
        Small Message(Event Player, Custom String("combo bonus"));
    }
}

rule ("reset combo on death") {
    event {
        Player Died;
        All;
        All;
    }
    actions {
        Set Player Variable(Victim, combo, 0);
    }
}

rule ("declare winner") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Compare(Event Player.score, >=, Global.target_score);
    }
    actions {
        Declare Player Victory(Event Player);
    }
}

rule ("spin while active") {
    event {
        Ongoing - Global;
    }
    conditions {
        Global.round_active == True;
    }
    actions {
        While(True);
            Modify Global Variable(target_score, Add, 0);
        End;
    }
}

rule ("award point subroutine") {
    event {
        Subroutine;
        award_point;
    }
    actions {
        Modify Player Variable(Event Player, score, Add, 1);
    }
}
