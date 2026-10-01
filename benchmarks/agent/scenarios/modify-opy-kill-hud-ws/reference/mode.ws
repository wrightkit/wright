settings
{
	main
	{
		Description: "Kill race"
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
        0: kills
}

rule ("force heroes") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        Has Spawned(Event Player) == True;
    }
    actions {
        Start Forcing Player To Be Hero(Event Player, Hero(Ana));
    }
}

rule ("count kill") {
    event {
        Player Earned Elimination;
        All;
        All;
    }
    conditions {
        Attacker != Victim;
    }
    actions {
        Modify Player Variable(Attacker, kills, Add, 1);
    }
}

rule ("restore health") {
    event {
        Player Earned Elimination;
        All;
        All;
    }
    actions {
        Heal(Attacker, Null, 50);
    }
}

rule ("kills display") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    actions {
        Create HUD Text(Event Player, Custom String("Eliminations: {0}", (Event Player).kills), Null, Null, Left, 0, Color(White), Null, Null, Visible To and String, Default Visibility);
    }
}

rule ("declare winner") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        (Event Player).kills >= 5;
    }
    actions {
        Declare Player Victory(Event Player);
    }
}
