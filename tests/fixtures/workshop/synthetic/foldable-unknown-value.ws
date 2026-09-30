settings
{
    main
    {
        Mode Name: "Synthetic"
    }
}

variables
{
    global:
        0: result
}

rule("probe")
{
    event
    {
        Ongoing - Global;
    }
    actions
    {
        Set Global Variable(result, sqrt(4));
    }
}
