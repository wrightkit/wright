variables {
    global:
        100: cakePos
        101: i2
        102: candlePos
}

rule ("cake") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(cakePos, Array(Vector(0.75, 0, 1.810660171779821), Vector(1.810660171779821, 0, 0.75), Vector(1.810660171779821, 0, -0.75), Vector(0.75, 0, -1.810660171779821), Vector(-0.75, 0, -1.810660171779821), Vector(-1.810660171779821, 0, -0.75), Vector(-1.810660171779821, 0, 0.75), Vector(-0.75, 0, 1.810660171779821)));
        Set Global Variable(candlePos, Empty Array);
        For Global Variable(i2, 0, 28, 1);
            Modify Global Variable(candlePos, Append To Array, Vector(Random Real(-1.280330085889910, 1.280330085889910), 0, Random Real(-1.280330085889910, 1.280330085889910)));
        End;
        For Global Variable(i2, 0, Count Of(Global.candlePos), 1);
            Create Beam Effect(All Players(All Teams), Grapple Beam, Add(Value In Array(Global.candlePos, Global.i2), Vector(0.001, 1, 0)), Add(Value In Array(Global.candlePos, Global.i2), Vector(0, 2, 0)), Color(White), Visible To);
        End;
        For Global Variable(i2, 0.125, 1, 0.125);
            Create Beam Effect(All Players(All Teams), Good Beam, Add(First Of(Global.cakePos), Vector(0, Global.i2, 0)), Add(Value In Array(Global.cakePos, 1), Vector(0, Global.i2, 0)), Color(Yellow), Visible To);
            Create Beam Effect(All Players(All Teams), Good Beam, Add(Value In Array(Global.cakePos, 1), Vector(0, Global.i2, 0)), Add(Value In Array(Global.cakePos, 2), Vector(0, Global.i2, 0)), Color(Yellow), Visible To);
            Create Beam Effect(All Players(All Teams), Good Beam, Add(Value In Array(Global.cakePos, 2), Vector(0, Global.i2, 0)), Add(Value In Array(Global.cakePos, 3), Vector(0, Global.i2, 0)), Color(Yellow), Visible To);
            Create Beam Effect(All Players(All Teams), Good Beam, Add(Value In Array(Global.cakePos, 3), Vector(0, Global.i2, 0)), Add(Value In Array(Global.cakePos, 4), Vector(0, Global.i2, 0)), Color(Yellow), Visible To);
            Create Beam Effect(All Players(All Teams), Good Beam, Add(Value In Array(Global.cakePos, 4), Vector(0, Global.i2, 0)), Add(Value In Array(Global.cakePos, 5), Vector(0, Global.i2, 0)), Color(Yellow), Visible To);
            Create Beam Effect(All Players(All Teams), Good Beam, Add(Value In Array(Global.cakePos, 5), Vector(0, Global.i2, 0)), Add(Value In Array(Global.cakePos, 6), Vector(0, Global.i2, 0)), Color(Yellow), Visible To);
            Create Beam Effect(All Players(All Teams), Good Beam, Add(Value In Array(Global.cakePos, 6), Vector(0, Global.i2, 0)), Add(Value In Array(Global.cakePos, 7), Vector(0, Global.i2, 0)), Color(Yellow), Visible To);
            Create Beam Effect(All Players(All Teams), Good Beam, Add(Value In Array(Global.cakePos, 7), Vector(0, Global.i2, 0)), Add(First Of(Global.cakePos), Vector(0, Global.i2, 0)), Color(Yellow), Visible To);
        End;
        For Global Variable(i2, -1.685660171779821, 1.810660171779821, 0.125);
            If(Compare(Absolute Value(Global.i2), >, 0.75));
                Create Beam Effect(All Players(All Teams), Good Beam, Add(Vector(Global.i2, 0, Subtract(1.810660171779821, Subtract(Absolute Value(Global.i2), 0.75))), Up), Add(Vector(Global.i2, 0, Multiply(-1, Subtract(1.810660171779821, Subtract(Absolute Value(Global.i2), 0.75)))), Up), Color(Yellow), Visible To);
            Else;
                Create Beam Effect(All Players(All Teams), Good Beam, Add(Vector(Global.i2, 0, 1.810660171779821), Up), Add(Vector(Global.i2, 0, -1.810660171779821), Up), Color(Yellow), Visible To);
            End;
        End;
    }
}

rule ("candles") {
    event {
        Ongoing - Global;
    }
    actions {
        While(True);
            Play Effect(All Players(All Teams), Bad Explosion, Color(Red), Add(Random Value In Array(Global.candlePos), Vector(0, 2.1, 0)), 0.1);
            Wait(0.016, Ignore Condition);
        End;
    }
}
