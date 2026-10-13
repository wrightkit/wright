variables {
    global:
        0: target_score
        1: match_started
    player:
        0: score
        1: streak
        2: bonus
}

rule ("configure match") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(target_score, 5);
        Set Global Variable(match_started, True);
    }
}

rule ("force soldier") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    actions {
        Start Forcing Player To Be Hero(Event Player, Hero(Soldier: 76));
    }
}

rule ("score on elimination") {
    event {
        Player Earned Elimination;
        All;
        All;
    }
    conditions {
        Global.match_started == True;
        Attacker != Victim;
    }
    actions {
        Modify Player Variable(Attacker, score, Add, 1);
        Modify Player Variable(Attacker, streak, Add, 1);
    }
}

rule ("reset streak on death") {
    event {
        Player Died;
        All;
        All;
    }
    actions {
        Set Player Variable(Victim, streak, 0);
    }
}

rule ("streak bonus points") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Event Player.streak >= 3;
    }
    actions {
        Modify Player Variable(Event Player, score, Add, 1);
        Set Player Variable(Event Player, streak, 0);
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
