settings
{
	main
	{
		Description: "Round demo"
	}
	modes
	{
		Deathmatch
		{
			enabled maps
			{
				Workshop Island
			}
		}
	}
}

variables {
    global:
        0: matchStarted
        1: roundTimer
        2: leader
        3: spareFlag
        4: bonusPoints
    player:
        0: kills
        1: deaths
        2: shield
}

subroutines {
    0: awardPoint
    1: refreshLeader
    2: resetTimer
}

rule ("awardPoint") {
    event {
        Subroutine;
        awardPoint;
    }
    actions {
        Modify Player Variable(Event Player, kills, Add, 1);
        Modify Global Variable(bonusPoints, Add, 1);
    }
}

rule ("refreshLeader") {
    event {
        Subroutine;
        refreshLeader;
    }
    actions {
        Set Global Variable(leader, First Of(Sorted Array(All Players(All Teams), Multiply(-1, (Current Array Element).kills))));
    }
}

rule ("resetTimer") {
    event {
        Subroutine;
        resetTimer;
    }
    actions {
        Set Global Variable(roundTimer, 300);
    }
}

rule ("start match") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(matchStarted, True);
        Call Subroutine(resetTimer);
    }
}

rule ("countdown") {
    event {
        Ongoing - Global;
    }
    conditions {
        Global.matchStarted == True;
    }
    actions {
        While(Compare(Global.roundTimer, >, 0));
            Wait(1, Ignore Condition);
            Modify Global Variable(roundTimer, Subtract, 1);
        End;
    }
}

rule ("restart on empty") {
    event {
        Ongoing - Global;
    }
    conditions {
        Number Of Players(All Teams) == 0;
    }
    actions {
        Call Subroutine(resetTimer);
    }
}

rule ("kill credit") {
    event {
        Player Earned Elimination;
        All;
        All;
    }
    actions {
        Call Subroutine(awardPoint);
        Call Subroutine(refreshLeader);
    }
}

rule ("death count") {
    event {
        Player Died;
        All;
        All;
    }
    actions {
        Modify Player Variable(Event Player, deaths, Add, 1);
    }
}

rule ("leader hud") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    actions {
        Create HUD Text(Event Player, Custom String("Leader: {0}", Global.leader), Null, Null, Top, 0, Color(White), Null, Null, Visible To and String, Default Visibility);
    }
}

rule ("spawn shield") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Has Spawned(Event Player) == True;
    }
    actions {
        Set Player Variable(Event Player, shield, 100);
    }
}
