variables {
    global:
        0: score
}

rule ("numbers") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(score, 5);
    }
}
