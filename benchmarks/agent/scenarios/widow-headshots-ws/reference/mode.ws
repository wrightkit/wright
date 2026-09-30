settings
{
	main
	{
		Mode Name: "Widowmaker Headshots"
		Description: "Headshots only. 10 kills in a row earns Infra-Sight."
	}
	modes
	{
		Deathmatch
		{
			enabled maps
			{
				Necropolis
				Château Guillard
				Castillo
			}
			Score To Win: 200
		}
	}
	heroes
	{
		General
		{
			Widowmaker
			{
				Passive Health Regeneration: Off
			}
			enabled heroes
			{
				Widowmaker
			}
		}
	}
}

variables {
    global:
        0: globalShots
        1: globalHeadshots
    player:
        0: kills
        1: streak
        2: shots
        3: headshots
        4: joinTime
        5: hpCap
        6: lastAmmo
        7: ignoreNextHit
        8: infraReady
        9: statsOn
        10: killfeedOn
        11: bigHud
}

rule ("Init globals") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(globalShots, 0);
        Set Global Variable(globalHeadshots, 0);
    }
}

rule ("Init player") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    actions {
        Start Forcing Player To Be Hero(Event Player, Hero(Widowmaker));
        Set Player Variable(Event Player, kills, 0);
        Set Player Variable(Event Player, streak, 0);
        Set Player Variable(Event Player, shots, 0);
        Set Player Variable(Event Player, headshots, 0);
        Set Player Variable(Event Player, ignoreNextHit, False);
        Set Player Variable(Event Player, infraReady, False);
        Set Player Variable(Event Player, statsOn, True);
        Set Player Variable(Event Player, killfeedOn, True);
        Set Player Variable(Event Player, bigHud, False);
        Set Player Variable(Event Player, joinTime, Total Time Elapsed);
    }
}

rule ("Spawn setup") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Is Alive(Event Player) == True;
    }
    actions {
        Set Player Variable(Event Player, hpCap, 9999);
        Set Max Health(Event Player, 100);
        Wait(0.1, Ignore Condition);
        If(Compare(Workshop Setting Integer(Custom String("Widowmaker"), Custom String("Widowmaker HP 󠀨0=Default 1=175 2=200 3=225󠀩"), 0, 0, 3, 0), ==, 1));
            Set Max Health(Event Player, Multiply(Divide(175, Max Health(Event Player)), 100));
        Else If(Compare(Workshop Setting Integer(Custom String("Widowmaker"), Custom String("Widowmaker HP 󠀨0=Default 1=175 2=200 3=225󠀩"), 0, 0, 3, 0), ==, 2));
            Set Max Health(Event Player, Multiply(Divide(200, Max Health(Event Player)), 100));
        Else If(Compare(Workshop Setting Integer(Custom String("Widowmaker"), Custom String("Widowmaker HP 󠀨0=Default 1=175 2=200 3=225󠀩"), 0, 0, 3, 0), ==, 3));
            Set Max Health(Event Player, Multiply(Divide(225, Max Health(Event Player)), 100));
        End;
        Wait(0.1, Ignore Condition);
        Heal(Event Player, Null, 9999);
        Wait(0.1, Ignore Condition);
        Set Player Variable(Event Player, hpCap, Health(Event Player));
        Set Player Variable(Event Player, lastAmmo, Ammo(Event Player, 0));
    }
}

rule ("Grappling Hook cooldown") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Ability Cooldown(Event Player, Button(Ability 1)) > 2.5;
    }
    actions {
        Set Ability Cooldown(Event Player, Button(Ability 1), 2.5);
    }
}

rule ("Refund non-headshot damage") {
    event {
        Player Took Damage;
        All;
        All;
    }
    actions {
        Abort If(Is Dead(Event Player));
        If((Event Player).ignoreNextHit);
            Set Player Variable(Event Player, ignoreNextHit, False);
            Set Player Variable(Event Player, hpCap, Health(Event Player));
            Abort;
        End;
        If(Event Was Critical Hit);
            Set Player Variable(Event Player, hpCap, Health(Event Player));
            Abort;
        End;
        Set Player Variable(Event Player, hpCap, Add(Health(Event Player), Event Damage));
        Heal(Event Player, Null, Event Damage);
    }
}

rule ("Block healthpacks") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Is Alive(Event Player) == True;
        Health(Event Player) > Add((Event Player).hpCap, 1);
    }
    actions {
        Set Player Variable(Event Player, ignoreNextHit, True);
        Damage(Event Player, Null, Subtract(Health(Event Player), (Event Player).hpCap));
    }
}

rule ("Remove damage falloff") {
    event {
        Player Dealt Damage;
        All;
        All;
    }
    conditions {
        Workshop Setting Toggle(Custom String("Widowmaker"), Custom String("Remove damage falloff"), True, 1) == True;
        Event Was Critical Hit == True;
        Is Button Held(Event Player, Button(Secondary Fire)) == False;
        Event Damage < 26;
    }
    actions {
        Abort If(Is Dead(Victim));
        Set Player Variable(Victim, ignoreNextHit, True);
        Damage(Victim, Event Player, Subtract(26, Event Damage));
    }
}

rule ("Count headshot hits") {
    event {
        Player Dealt Damage;
        All;
        All;
    }
    conditions {
        Event Was Critical Hit == True;
    }
    actions {
        Modify Player Variable(Event Player, headshots, Add, 1);
        Modify Global Variable(globalHeadshots, Add, 1);
    }
}

rule ("Count shots (ammo spent)") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Ammo(Event Player, 0) < (Event Player).lastAmmo;
    }
    actions {
        Modify Player Variable(Event Player, shots, Add, Subtract((Event Player).lastAmmo, Ammo(Event Player, 0)));
        Modify Global Variable(globalShots, Add, Subtract((Event Player).lastAmmo, Ammo(Event Player, 0)));
        Set Player Variable(Event Player, lastAmmo, Ammo(Event Player, 0));
    }
}

rule ("Track ammo refill") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Ammo(Event Player, 0) > (Event Player).lastAmmo;
    }
    actions {
        Set Player Variable(Event Player, lastAmmo, Ammo(Event Player, 0));
    }
}

rule ("Elimination") {
    event {
        Player Earned Elimination;
        All;
        All;
    }
    actions {
        Abort If(Compare(Victim, ==, Event Player));
        Modify Player Variable(Event Player, kills, Add, 1);
        Modify Player Variable(Event Player, streak, Add, 1);
        Set Player Variable(Event Player, hpCap, Min(Max Health(Event Player), Add((Event Player).hpCap, Multiply(Max Health(Victim), 0.25))));
        Heal(Event Player, Null, Multiply(Max Health(Victim), 0.25));
        If(Compare((Event Player).streak, ==, 10));
            Set Player Variable(Event Player, infraReady, True);
            Set Ultimate Charge(Event Player, 100);
            Small Message(Event Player, Custom String("Infra-Sight ready!"));
        End;
        If(Compare((Event Player).kills, >=, Workshop Setting Integer(Custom String("Match"), Custom String("Kills to win"), 50, 1, 200, 0)));
            Declare Player Victory(Event Player);
    }
}

rule ("Death resets streak") {
    event {
        Player Died;
        All;
        All;
    }
    actions {
        Set Player Variable(Event Player, streak, 0);
        Set Player Variable(Event Player, infraReady, False);
        Set Ultimate Charge(Event Player, 0);
    }
}

rule ("No natural ultimate charge") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Ultimate Charge Percent(Event Player) > 0;
        (Event Player).infraReady == False;
    }
    actions {
        Set Ultimate Charge(Event Player, 0);
    }
}

rule ("Infra-Sight duration") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Is Using Ultimate(Event Player) == True;
    }
    actions {
        Wait(10, Abort When False);
        Set Player Variable(Event Player, infraReady, False);
        Set Ultimate Charge(Event Player, 0);
        Set Ultimate Ability Enabled(Event Player, False);
        Wait(0.25, Ignore Condition);
        Set Ultimate Ability Enabled(Event Player, True);
    }
}

rule ("HUD toggles") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Is Button Held(Event Player, Button(Interact)) == True;
    }
    actions {
        If(Is Button Held(Event Player, Button(Crouch)));
            Set Player Variable(Event Player, killfeedOn, Not((Event Player).killfeedOn));
            If((Event Player).killfeedOn);
                Enable Kill Feed(Event Player);
            Else;
                Disable Kill Feed(Event Player);
            End;
        Else If(Is Button Held(Event Player, Button(Melee)));
            Set Player Variable(Event Player, bigHud, Not((Event Player).bigHud));
        Else;
            Set Player Variable(Event Player, statsOn, Not((Event Player).statsOn));
        End;
        Wait Until(Not(Is Button Held(Event Player, Button(Interact))), 5);
    }
}

rule ("HUD: personal stats") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    actions {
        Create HUD Text(Event Player, If-Then-Else((Event Player).statsOn, Custom String("Kills {0}/{1}  |  Streak {2}", (Event Player).kills, Workshop Setting Integer(Custom String("Match"), Custom String("Kills to win"), 50, 1, 200, 0), (Event Player).streak), Custom String("")), Null, Null, Left, 1, Color(White), Null, Null, Visible To and String, Default Visibility);
        Create HUD Text(Event Player, Null, Null, If-Then-Else((Event Player).statsOn, Custom String("Time {0}s  |  {1} kills/min", Round To Integer(Subtract(Total Time Elapsed, (Event Player).joinTime), To Nearest), Divide(Round To Integer(Multiply(Divide((Event Player).kills, Max(1, Divide(Subtract(Total Time Elapsed, (Event Player).joinTime), 60))), 10), To Nearest), 10)), Custom String("")), Left, 2, Null, Null, Color(Sky Blue), Visible To and String, Default Visibility);
        Create HUD Text(Event Player, Null, Null, If-Then-Else(And((Event Player).statsOn, Compare((Event Player).kills, >, 0)), Custom String("Pace: {0}s to {1} kills", Round To Integer(Multiply(Divide(Max(0, Subtract(Workshop Setting Integer(Custom String("Match"), Custom String("Kills to win"), 50, 1, 200, 0), (Event Player).kills)), Max(0.01, Divide((Event Player).kills, Max(1, Divide(Subtract(Total Time Elapsed, (Event Player).joinTime), 60))))), 60), To Nearest), Workshop Setting Integer(Custom String("Match"), Custom String("Kills to win"), 50, 1, 200, 0)), Custom String("")), Left, 3, Null, Null, Color(Sky Blue), Visible To and String, Default Visibility);
        Create HUD Text(Event Player, Null, Null, If-Then-Else((Event Player).statsOn, Custom String("Your accuracy {0}%", Round To Integer(Multiply(Divide((Event Player).headshots, Max(1, (Event Player).shots)), 100), To Nearest)), Custom String("")), Left, 4, Null, Null, Color(Sky Blue), Visible To and String, Default Visibility);
    }
}

rule ("HUD: global stats") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    actions {
        Create HUD Text(Event Player, If-Then-Else(And((Event Player).statsOn, (Event Player).bigHud), Custom String("Global accuracy {0}%", Round To Integer(Multiply(Divide(Global.globalHeadshots, Max(1, Global.globalShots)), 100), To Nearest)), Custom String("")), Null, Null, Left, 5, Color(White), Null, Null, Visible To and String, Default Visibility);
        Create HUD Text(Event Player, Null, Null, If-Then-Else(And((Event Player).statsOn, (Event Player).bigHud), Custom String("Streak leaders"), Custom String("")), Left, 6, Null, Null, Color(Orange), Visible To and String, Default Visibility);
        Create HUD Text(Event Player, Null, Null, If-Then-Else(And(And((Event Player).statsOn, (Event Player).bigHud), Compare(Number Of Players(All Teams), >=, 1)), Custom String("1. {0}: {1}", First Of(Sorted Array(All Players(All Teams), Multiply(-1, (Current Array Element).streak))), (First Of(Sorted Array(All Players(All Teams), Multiply(-1, (Current Array Element).streak)))).streak), Custom String("")), Left, 7, Null, Null, Color(Orange), Visible To and String, Default Visibility);
        Create HUD Text(Event Player, Null, Null, If-Then-Else(And(And((Event Player).statsOn, (Event Player).bigHud), Compare(Number Of Players(All Teams), >=, 2)), Custom String("2. {0}: {1}", Value In Array(Sorted Array(All Players(All Teams), Multiply(-1, (Current Array Element).streak)), 1), (Value In Array(Sorted Array(All Players(All Teams), Multiply(-1, (Current Array Element).streak)), 1)).streak), Custom String("")), Left, 8, Null, Null, Color(Orange), Visible To and String, Default Visibility);
        Create HUD Text(Event Player, Null, Null, If-Then-Else(And(And((Event Player).statsOn, (Event Player).bigHud), Compare(Number Of Players(All Teams), >=, 3)), Custom String("3. {0}: {1}", Value In Array(Sorted Array(All Players(All Teams), Multiply(-1, (Current Array Element).streak)), 2), (Value In Array(Sorted Array(All Players(All Teams), Multiply(-1, (Current Array Element).streak)), 2)).streak), Custom String("")), Left, 9, Null, Null, Color(Orange), Visible To and String, Default Visibility);
    }
}

rule ("HUD: controls hint") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    actions {
        Create HUD Text(Event Player, Null, Null, Custom String("Interact: stats | Crouch+Interact: killfeed | Melee+Interact: big HUD"), Right, 1, Null, Null, Color(Gray), Visible To and String, Default Visibility);
    }
}
