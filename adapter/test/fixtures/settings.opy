settings {
    "main": {
        "description": "settings mini-fixture"
    },
    "gamemodes": {
        "skirmish": {
            "enabled": false,
            "enabledMaps": [
                "workshopIsland"
            ]
        },
        "general": {
            "heroLimit": "off",
            "respawnTime%": 30,
            "enableHeroSwitching": true
        }
    }
}

rule "setup":
    @Event global
    disableInspector()
