settings
{
    main
    {
        Mode Name: "Synthetic"
        zzWrightSyntheticUnknownKey: 1
    }
}

rule("probe")
{
    event
    {
        Ongoing - Global;
    }
}
