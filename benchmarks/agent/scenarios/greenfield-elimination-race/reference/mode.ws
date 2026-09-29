variables {
    global:
        0: target_score
    player:
        0: score
}

rule ("configure match") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(target_score, 7);
        Create HUD Text(All Players(All Teams), Custom String("First to {0}", Global.target_score), Null, Null, Top, 0, Color(White), Color(White), Color(White), Visible To and String, Default Visibility);
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
        Attacker != Victim;
    }
    actions {
        Modify Player Variable(Attacker, score, Add, 1);
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
