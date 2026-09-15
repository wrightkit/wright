variables {
    global:
        0: position
}

rule ("cake") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(position, Vector(0.75, 0, 1));
    }
}
