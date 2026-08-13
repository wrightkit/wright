//! Model-level tests for the internal Opy HIR and Workshop IR: direct
//! construction, validation, deterministic dumps, and rejection of invalid
//! states.

use wright_ir::error::IrError;
use wright_ir::hir::{BinaryOp, Expr, GlobalVar, Program as HirProgram, Rule, Stmt};
use wright_ir::ids::Id;
use wright_ir::source::{Position, SourceFile, Span};
use wright_ir::wir::{self, Action, Program as WirProgram};

fn span(file: Id<SourceFile>, start_line: u32, start_col: u32, end_col: u32) -> Span {
    Span::new(
        file,
        Position::new(start_line, start_col),
        Position::new(start_line, end_col),
    )
}

/// Build a small HIR program directly: one global `index`, one rule with a
/// `while (index < 3) { index = index + 1 }` body.
fn build_hir_program() -> HirProgram {
    let mut program = HirProgram::default();
    let file = program.files.push(SourceFile::new("source.opy"));
    let s = |line, col, end| span(file, line, col, end);

    let index = program.globals.push(GlobalVar {
        name: "index".into(),
        index: None,
        span: Some(s(1, 1, 12)),
        name_span: Some(s(1, 10, 15)),
        initializer: None,
    });

    let one = program.exprs.push(Expr::Number {
        value: 1.0,
        text: "1".into(),
        span: Some(s(3, 18, 20)),
    });
    let index_ref = program.exprs.push(Expr::GlobalVar {
        variable: index,
        span: Some(s(3, 9, 15)),
    });
    let add = program.exprs.push(Expr::Binary {
        op: BinaryOp::Add,
        left: index_ref,
        right: one,
        span: Some(s(3, 9, 20)),
    });
    let three = program.exprs.push(Expr::Number {
        value: 3.0,
        text: "3".into(),
        span: Some(s(2, 19, 21)),
    });
    let cond = program.exprs.push(Expr::Binary {
        op: BinaryOp::Less,
        left: index_ref,
        right: three,
        span: Some(s(2, 11, 21)),
    });
    let assign = program.stmts.push(Stmt::Assign {
        target: index_ref,
        value: add,
        span: Some(s(3, 9, 20)),
    });
    let while_stmt = program.stmts.push(Stmt::While {
        condition: cond,
        body: vec![assign],
        span: Some(s(2, 5, 11)),
    });
    program.rules.push(Rule {
        name: "loop".into(),
        span: Some(s(1, 1, 6)),
        name_span: Some(s(1, 6, 7)),
        disabled: false,
        event: wright_ir::hir::Event {
            name: "global".into(),
            args: vec![],
            span: Some(s(1, 1, 2)),
        },
        conditions: vec![],
        actions: vec![while_stmt],
    });
    program
}

#[test]
fn hir_model_validates_and_dumps_deterministically() {
    let program = build_hir_program();
    program
        .validate()
        .expect("directly built HIR must validate");

    let first = program.dump();
    let second = program.dump();
    assert_eq!(first, second);
    assert!(first.contains("globalVariable index"));
    assert!(first.contains("while (index < 3)"));
    assert!(first.contains("assign index = (index + 1)"));
}

#[test]
fn hir_model_rejects_dangling_statement_reference() {
    let mut program = build_hir_program();
    let rule = program
        .rules
        .get_mut(wright_ir::hir::RuleId::from_index(0))
        .unwrap();
    // Reference a statement id that was never pushed.
    rule.actions = vec![Id::from_index(rule.actions.len() + 999)];
    let error = program.validate().unwrap_err();
    assert_eq!(error.code(), "dangling-reference");
    assert!(matches!(error, IrError::DanglingReference { .. }));
}

#[test]
fn hir_model_rejects_dangling_symbol_reference() {
    let mut program = build_hir_program();
    let rule = program
        .rules
        .get_mut(wright_ir::hir::RuleId::from_index(0))
        .unwrap();
    // The while condition now references an out-of-range expression.
    let condition = program.exprs.push(Expr::GlobalVar {
        variable: Id::from_index(usize::MAX),
        span: None,
    });
    if let Stmt::While {
        condition: cond, ..
    } = program.stmts.get_mut(rule.actions[0]).unwrap()
    {
        *cond = condition;
    }
    let error = program.validate().unwrap_err();
    assert_eq!(error.code(), "dangling-reference");
}

/// Build a small Workshop IR program directly: one global variable, one rule
/// with a `Set Global Variable` action.
fn build_wir_program() -> WirProgram {
    let mut program = WirProgram::default();
    let file = program.files.push(SourceFile::new("source.opy"));

    let global = program.global_variables.push(wir::WorkshopVariable {
        name: "x".into(),
        index: 0,
        span: Some(span(file, 1, 1, 12)),
        name_span: Some(span(file, 1, 10, 11)),
        initializer: None,
    });
    let value = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Number(5.0),
        None,
    ));
    let action = program.actions.push(Action::SetGlobalVariable {
        variable: global,
        value,
        span: Some(span(file, 2, 5, 24)),
        target_span: Some(span(file, 2, 5, 6)),
    });
    program.rules.push(wir::Rule {
        name: "init".into(),
        span: Some(span(file, 2, 1, 6)),
        name_span: Some(span(file, 2, 5, 6)),
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![],
        actions: vec![action],
    });
    program
}

#[test]
fn wir_model_validates_and_dumps_deterministically() {
    let program = build_wir_program();
    program
        .validate()
        .expect("directly built WIR must validate");

    let first = program.dump();
    let second = program.dump();
    assert_eq!(first, second);
    assert!(first.contains("global variables:"));
    assert!(first.contains("x (index 0)"));
    assert!(first.contains("setGlobalVariable x = 5"));
}

#[test]
fn wir_model_rejects_dangling_action_reference() {
    let mut program = build_wir_program();
    let rule = program.rules.get_mut(wir::RuleId::from_index(0)).unwrap();
    rule.actions = vec![Id::from_index(usize::MAX)];
    let error = program.validate().unwrap_err();
    assert_eq!(error.code(), "dangling-reference");
}

#[test]
fn wir_model_rejects_dangling_variable_reference() {
    let mut program = build_wir_program();
    let action = program
        .actions
        .get_mut(wir::ActionId::from_index(0))
        .unwrap();
    if let Action::SetGlobalVariable { variable, .. } = action {
        *variable = Id::from_index(999);
    }
    let error = program.validate().unwrap_err();
    assert_eq!(error.code(), "dangling-reference");
}
