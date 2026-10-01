settings
{
	main
	{
		Description: "Score race"
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

rule ("show score") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    actions {
        Create HUD Text(Event Player, Custom String("Score: {0}", (Event Player).score), Null, Null, Left, 0, Color(White), Null, Null, Visible To and String, Default Visibility);
    }
}

rule ("declare winner") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    conditions {
        (Event Player).score >= 10;
    }
    actions {
        Declare Player Victory(Event Player);
    }
}
