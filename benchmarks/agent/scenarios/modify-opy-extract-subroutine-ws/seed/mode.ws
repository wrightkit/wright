settings
{
	main
	{
		Description: "Reward demo"
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
    player:
        0: streak
}

rule ("reward on elimination") {
    event {
        Player Earned Elimination;
        All;
        All;
    }
    actions {
        Modify Player Variable(Attacker, streak, Add, 1);
        Set Ultimate Charge(Attacker, 100);
        Big Message(Attacker, Custom String("Streak {0}", (Attacker).streak));
        Heal(Attacker, Null, 50);
    }
}

rule ("reward on assist") {
    event {
        Player Dealt Final Blow;
        All;
        All;
    }
    actions {
        Modify Player Variable(Attacker, streak, Add, 1);
        Set Ultimate Charge(Attacker, 100);
        Big Message(Attacker, Custom String("Streak {0}", (Attacker).streak));
        Heal(Attacker, Null, 50);
    }
}

rule ("reset streak") {
    event {
        Player Died;
        All;
        All;
    }
    actions {
        Set Player Variable(Event Player, streak, 0);
    }
}
