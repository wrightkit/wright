variables {
    global:
        0: result
        1: values
}

rule ("expressions") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(values, Array(1, 2, 3));
        Set Global Variable(result, Add(Count Of(Global.values), 1));
    }
}
