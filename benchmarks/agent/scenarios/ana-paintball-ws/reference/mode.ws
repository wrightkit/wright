settings
{
	main
	{
		Mode Name: "Ana Paintball"
		Description: "Every Ana rifle shot kills. Scoped shots pierce. Sleep darts + melee kill (and cost the victim a point). 5/10/15 streaks, nuke at 15, dart-only finish at 24. First to 25."
	}
	modes
	{
		Deathmatch
		{
			enabled maps
			{
				Black Forest
				Château Guillard
				Eichenwalde
				Kanezaka
			}
			Score To Win: 25
		}
	}
}

variables {
    global:
        0: pierceEye
        1: pierceDir
        2: pierceWall
        3: pierceCandidates
        4: pierceTarget
        5: pierceIdx
        6: pierceAlong
        7: pierceOnRay
    player:
        0: streak
        1: nukeActive
        2: nukeFired
}

rule ("Force Ana") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Has Spawned(Event Player) == True;
        (Event Player).nukeActive == False;
    }
    actions {
        Start Forcing Player To Be Hero(Event Player, Hero(Ana));
    }
}

rule ("Rifle hit kills") {
    event {
        Player Dealt Damage;
        All;
        All;
    }
    conditions {
        Event Ability == Button(Primary Fire);
        Attacker != Victim;
        (Attacker).nukeActive == False;
        Score Of(Attacker) < 24;
    }
    actions {
        Kill(Victim, Attacker);
        "Scoped: also kill every other enemy on the line of fire up to the first wall.\nNo waits in here, so the shared scratch globals cannot interleave between shots."
        If(Is Firing Secondary(Attacker));
            Set Global Variable(pierceEye, Eye Position(Attacker));
            Set Global Variable(pierceDir, Facing Direction Of(Attacker));
            Set Global Variable(pierceWall, Distance Between(Global.pierceEye, Ray Cast Hit Position(Global.pierceEye, Add(Global.pierceEye, Multiply(Global.pierceDir, 200)), Empty Array, Array(Attacker), False)));
            Set Global Variable(pierceCandidates, All Living Players(All Teams));
            For Global Variable(pierceIdx, 0, Count Of(Global.pierceCandidates), 1);
                Set Global Variable(pierceTarget, Value In Array(Global.pierceCandidates, Global.pierceIdx));
                Skip If(Compare(Global.pierceTarget, ==, Attacker), 6);
                Set Global Variable(pierceAlong, Dot Product(Subtract(Add(Position Of(Global.pierceTarget), Vector(0, 0.9, 0)), Global.pierceEye), Global.pierceDir));
                Skip If(Or(Compare(Global.pierceAlong, <, 0), Compare(Global.pierceAlong, >, Global.pierceWall)), 4);
                Set Global Variable(pierceOnRay, Add(Global.pierceEye, Multiply(Global.pierceDir, Global.pierceAlong)));
                If(Compare(Min(Min(Distance Between(Add(Position Of(Global.pierceTarget), Vector(0, 0.3, 0)), Global.pierceOnRay), Distance Between(Add(Position Of(Global.pierceTarget), Vector(0, 0.9, 0)), Global.pierceOnRay)), Distance Between(Eye Position(Global.pierceTarget), Global.pierceOnRay)), <=, 0.6));
                    Kill(Global.pierceTarget, Attacker);
                End;
                //__label_continue_2__:
                //__label_continue_3__:
            End;
    }
}

rule ("Melee kills sleeper") {
    event {
        Player Dealt Damage;
        All;
        All;
    }
    conditions {
        Event Ability == Button(Melee);
        Attacker != Victim;
        Has Status(Victim, Asleep) == True;
    }
    actions {
        Kill(Victim, Attacker);
        Modify Player Score(Victim, -1);
    }
}

rule ("Death bookkeeping") {
    event {
        Player Died;
        All;
        All;
    }
    actions {
        Set Player Variable(Event Player, streak, 0);
        If((Event Player).nukeActive);
            Set Player Variable(Event Player, nukeActive, False);
            Set Player Variable(Event Player, nukeFired, False);
    }
}

rule ("Count kill") {
    event {
        Player Died;
        All;
        All;
    }
    conditions {
        Attacker != Null;
        Attacker != Event Player;
    }
    actions {
        Modify Player Variable(Attacker, streak, Add, 1);
        If(Compare((Attacker).streak, ==, 5));
            Big Message(All Players(All Teams), Custom String("{0} is on a RAMPAGE! 5 kills in a row", Attacker));
        Else If(Compare((Attacker).streak, ==, 10));
            Big Message(All Players(All Teams), Custom String("{0} is UNSTOPPABLE! 10 kills in a row", Attacker));
        Else If(Compare((Attacker).streak, ==, 15));
            Big Message(All Players(All Teams), Custom String("{0} is GODLIKE! 15 kills in a row - NUCLEAR STRIKE incoming", Attacker));
    }
}

rule ("Grant nuke") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        (Event Player).streak == 15;
        (Event Player).nukeActive == False;
        Is Alive(Event Player) == True;
    }
    actions {
        Set Player Variable(Event Player, nukeActive, True);
        Set Player Variable(Event Player, nukeFired, False);
        Start Forcing Player To Be Hero(Event Player, Hero(Doomfist));
        Wait(0.25, Ignore Condition);
        Set Primary Fire Enabled(Event Player, False);
        Set Secondary Fire Enabled(Event Player, False);
        Set Ability 1 Enabled(Event Player, False);
        Set Ability 2 Enabled(Event Player, False);
        Set Melee Enabled(Event Player, False);
        Set Ultimate Charge(Event Player, 100);
        Big Message(Event Player, Custom String("NUCLEAR STRIKE READY - use your ultimate!"));
    }
}

rule ("Nuke fired") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        (Event Player).nukeActive != False;
        Is Using Ultimate(Event Player) == True;
    }
    actions {
        Set Player Variable(Event Player, nukeFired, True);
    }
}

rule ("Nuke kill wins") {
    event {
        Player Died;
        All;
        All;
    }
    conditions {
        Attacker != Null;
        Attacker != Event Player;
        (Attacker).nukeActive != False;
        Or((Attacker).nukeFired, Compare(Event Ability, ==, Button(Ultimate))) == True;
    }
    actions {
        Declare Player Victory(Attacker);
    }
}

rule ("Nuke spent") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        (Event Player).nukeActive != False;
        (Event Player).nukeFired != False;
        Is Using Ultimate(Event Player) == False;
    }
    actions {
        "Let late meteor kills register before handing the rifle back."
        Wait(1.5, Abort When False);
        Set Player Variable(Event Player, nukeActive, False);
        Set Player Variable(Event Player, nukeFired, False);
        Start Forcing Player To Be Hero(Event Player, Hero(Ana));
        Wait(0.25, Ignore Condition);
        Set Primary Fire Enabled(Event Player, True);
        Set Secondary Fire Enabled(Event Player, True);
        Set Ability 1 Enabled(Event Player, True);
        Set Ability 2 Enabled(Event Player, True);
        Set Melee Enabled(Event Player, True);
    }
}

rule ("Dart-only at 24") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Has Spawned(Event Player) == True;
    }
    actions {
        While(True);
            Set Primary Fire Enabled(Event Player, And(Compare(Score Of(Event Player), <, 24), Not((Event Player).nukeActive)));
            Wait(0.25, Ignore Condition);
        End;
    }
}

rule ("Fast dart at 24") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Score Of(Event Player) >= 24;
        Ability Cooldown(Event Player, Button(Ability 1)) > 2.5;
    }
    actions {
        Set Ability Cooldown(Event Player, Button(Ability 1), 2.5);
    }
}

rule ("HUD") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    actions {
        Create HUD Text(Event Player, Custom String("Streak {0}", (Event Player).streak), Null, Null, Left, 0, Color(White), Null, Null, Visible To and String, Default Visibility);
        Create HUD Text(If-Then-Else(Compare(Score Of(Event Player), >=, 24), Event Player, Null), Null, Custom String("Rifle OFF - land a sleep dart, then melee"), Null, Left, 1, Null, Color(Red), Null, Visible To and String, Default Visibility);
    }
}
