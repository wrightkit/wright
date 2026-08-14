//! Matrix-surface emission closure scan (#87 AC-17).
//!
//! For every construct family on the matrix-listed supported
//! surface, the native emitter's spelling must self-round-trip:
//! parse(emit(x)) reparses and re-emits byte-identically. Each case
//! below is a real `wright compile --profile compat` artifact of a
//! minimal supported-surface program; the scan asserts the emitted
//! spelling is a byte-identical fixed point of the ws parser/
//! emitter. Oracle-canonical spellings pinned by repros are
//! asserted in the emitter tests (AC-15/16).

use wright_workshop::catalog::{Catalog, Locale};
use wright_workshop::emitter;
use wright_workshop::parser;

fn catalog() -> Catalog {
    Catalog::builtin().unwrap()
}

fn en() -> Locale {
    Locale::new("en-US")
}

/// Parse an emitted artifact, re-emit, and assert byte-identity.
fn assert_closure(label: &str, artifact: &str) {
    let program = parser::parse(artifact, &catalog(), &en())
        .unwrap_or_else(|error| panic!("{label} must reparse: {error}"));
    let reemitted = emitter::emit(&program, &catalog(), &en())
        .unwrap_or_else(|error| panic!("{label} must re-emit: {error}"));
    assert_eq!(
        artifact, reemitted,
        "{label} emitted spelling must be a byte-identical fixed point"
    );
}

const ASSIGN_AUG: &str = "variables {\n    global:\n        0: g\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(g, 1);\n        Modify Global Variable(g, Add, 2);\n        Modify Global Variable(g, Subtract, 1);\n        Modify Global Variable(g, Multiply, 2);\n    }\n}\n\n";
const CALLS: &str = "variables {\n    global:\n        0: g\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(g, Count Of(Array(1, 2)));\n        Set Global Variable(g, 3);\n        Set Global Variable(g, 2);\n        Wait(1, Ignore Condition);\n    }\n}\n\n";
const CONDITIONS: &str = "variables {\n    global:\n        0: x\n    player:\n        0: p\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Each Player;\n        All;\n        All;\n    }\n    conditions {\n        (Event Player).p > 1;\n        Global.x == 0;\n    }\n    actions {\n        Disable Inspector Recording;\n    }\n}\n\n";
const DECL_INDEX: &str = "variables {\n    global:\n        3: x\n    player:\n        1: y\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Disable Inspector Recording;\n    }\n}\n\n";
const DECL_NUMS: &str = "variables {\n    global:\n        0: j\n        1: h\n        2: k\n    player:\n        0: p\n}\n\nrule (\"Initialize global variables\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(j, 5);\n        Set Global Variable(k, 0.0);\n    }\n}\n\nrule (\"Initialize player variables\") {\n    event {\n        Ongoing - Each Player;\n        All;\n        All;\n    }\n    actions {\n        Set Player Variable(Event Player, p, 7);\n    }\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Disable Inspector Recording;\n    }\n}\n\n";
const EXPR_ESCAPES: &str = "variables {\n    global:\n        0: g\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(g, Custom String(\"a\\nb\tc\\\"d\"));\n    }\n}\n\n";
const EXPR_LITERALS: &str = "variables {\n    global:\n        0: g\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(g, Array(1, 2, 3));\n        Set Global Variable(g, Array(Custom String(\"a\"), Custom String(\"b\")));\n        Set Global Variable(g, Vector(1.5, -2, 3));\n        Set Global Variable(g, True);\n        Set Global Variable(g, 0.5);\n    }\n}\n\n";
const EXPR_LONG: &str = "variables {\n    global:\n        0: g\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(g, Custom String(\"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB{0}\", Custom String(\"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB{0}\", Custom String(\"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB\"))));\n    }\n}\n\n";
const FORMAT_FOLD: &str = "variables {\n    global:\n        0: z\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(z, Custom String(\"v: 3\"));\n    }\n}\n\n";
const FORMAT_PARTIAL: &str = "variables {\n    global:\n        0: x\n        1: z\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(z, Custom String(\"3 {0}\", Global.x));\n    }\n}\n\n";
const FORMAT_VAR: &str = "variables {\n    global:\n        0: x\n        1: z\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(z, Custom String(\"v: {0}\", Global.x));\n    }\n}\n\n";
const IF_ELSE: &str = "variables {\n    global:\n        0: x\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        If(Compare(Global.x, ==, 1));\n            Disable Inspector Recording;\n        Else If(Compare(Global.x, ==, 2));\n            Disable Inspector Recording;\n        Else;\n            Disable Inspector Recording;\n    }\n}\n\n";
const IF_FINAL: &str = "variables {\n    global:\n        0: x\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        If(Compare(Global.x, ==, 1));\n            Disable Inspector Recording;\n    }\n}\n\n";
const LOOPS: &str = "variables {\n    global:\n        0: i\n        1: g\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        For Global Variable(i, 0, 3, 1);\n            Set Global Variable(g, Global.i);\n        End;\n        While(Compare(Global.g, <, 3));\n            Modify Global Variable(g, Add, 1);\n            Wait(1, Ignore Condition);\n        End;\n    }\n}\n\n";
const MACRO: &str = "variables {\n    global:\n        0: g\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(g, 6);\n    }\n}\n\n";
const PLAYERVAR_READ: &str = "variables {\n    global:\n        0: g\n    player:\n        0: p\n        1: q\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Each Player;\n        All;\n        All;\n    }\n    actions {\n        Set Global Variable(g, (Event Player).p);\n        If(Compare(Add((Event Player).p, (Event Player).q), >, 1));\n            Disable Inspector Recording;\n    }\n}\n\n";
const SUBROUTINE: &str = "subroutines {\n    0: foo\n    1: bar\n}\n\nrule (\"Subroutine bar\") {\n    event {\n        Subroutine;\n        bar;\n    }\n    actions {\n        Disable Inspector Recording;\n    }\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Call Subroutine(foo);\n    }\n}\n\n";

const PVMOD: &str = "variables {\n    player:\n        0: p\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Each Player;\n        All;\n        All;\n    }\n    actions {\n        Modify Player Variable(Event Player, p, Add, 2);\n        Modify Player Variable(Event Player, p, Subtract, 1);\n        Modify Player Variable(Event Player, p, Multiply, 3);\n        Modify Player Variable(Event Player, p, Divide, 2);\n        Modify Player Variable(Event Player, p, Modulo, 5);\n    }\n}\n\n";

#[test]
fn emitted_spellings_self_round_trip_across_the_matrix_surface() {
    assert_closure("assign_aug", ASSIGN_AUG);
    assert_closure("calls", CALLS);
    assert_closure("conditions", CONDITIONS);
    assert_closure("pvmod", PVMOD);
    assert_closure("decl_index", DECL_INDEX);
    assert_closure("decl_nums", DECL_NUMS);
    assert_closure("expr_escapes", EXPR_ESCAPES);
    assert_closure("expr_literals", EXPR_LITERALS);
    assert_closure("expr_long", EXPR_LONG);
    assert_closure("format_fold", FORMAT_FOLD);
    assert_closure("format_partial", FORMAT_PARTIAL);
    assert_closure("format_var", FORMAT_VAR);
    assert_closure("if_else", IF_ELSE);
    assert_closure("if_final", IF_FINAL);
    assert_closure("loops", LOOPS);
    assert_closure("macro", MACRO);
    assert_closure("playervar_read", PLAYERVAR_READ);
    assert_closure("subroutine", SUBROUTINE);
}
