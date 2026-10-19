variables {
    global:
        0: score
    player:
        1: hp
}

subroutines {
    0: shared
}

disabled rule ("off") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(score, 0);
    }
}

rule ("setup") {
    event {
        Ongoing - Global;
    }
    conditions {
        Global.score < 10;
        disabled Global.score > 0;
    }
    actions {
        If(Global.score == 0);
            Set Global Variable(score, 1);
        Else;
            Call Subroutine(shared);
        End;
        While(Global.score < 3);
            Modify Global Variable(score, Add, 1);
            Wait(0.016, Ignore Condition);
        End;
    }
}

rule ("players") {
    event {
        Ongoing - Each Player;
        Team 1;
        Slot 0;
    }
    actions {
        Set Player Variable(Event Player, hp, 100);
        disabled Wait(1, Ignore Condition);
    }
}

// `None` in an unpinned position stays an ambiguous enum the
// rule-content-v1 export cannot express: the service refuses this rule's
// content and the view renders the refusal in place.
rule ("opaque") {
    event {
        Ongoing - Global;
    }
    actions {
        Small Message(All Players(Team 1), Custom String("score: {0}", None));
    }
}

rule ("after") {
    event {
        Ongoing - Global;
    }
    actions {
        Wait(1, Ignore Condition);
    }
}

rule ("Subroutine shared") {
    event {
        Subroutine;
        shared;
    }
    actions {
        Set Global Variable(score, 9);
    }
}
