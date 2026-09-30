settings
{
	main
	{
		Description: "First to five"
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
        0: score
}

rule ("score on elimination") {
    event {
        Player Earned Elimination;
        All;
        All;
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
        (Event Player).score >= 5;
    }
    actions {
        Declare Player Victory(Event Player);
    }
}
