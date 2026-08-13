//! Semantic resolution and HIR lowering (#45).
//!
//! Resolves the parsed CST into the existing Wright-owned Opy HIR contract
//! (`wright_core::hir::Program`): declarations and references resolve to
//! typed HIR nodes, custom enums fold to constants, `vect` becomes a vector,
//! `.format()` becomes a format node, `wait` default arguments are filled,
//! and subroutine calls become `CallSubroutine` statements. Semantic errors
//! (unknown identifiers, unknown enum members, invalid `vect` arity) are
//! structured and source-located.

use std::collections::{HashMap, HashSet};

use wright_core::hir::types::{
    Declaration, Define, Event, Expr as HirExpr, Generator, IfBranch, Position,
    Program as HirProgram, Protocol, Rule, RuleEntry, Settings as HirSettings,
    SettingsNode as HirSettingsNode, SourceFile, Span as HirSpan, Stmt as HirStmt,
};

use crate::cst::{self, Decl, Expr, RuleEntry as CstRuleEntry, Stmt};
use crate::diag::{FrontendError, FrontendResult, Span};

/// The protocol envelope this frontend produces.
const PROTOCOL_NAME: &str = "wright/opy-hir";
const PROTOCOL_VERSION: &str = "1.1.0";

/// Corpus-evidenced `.opy` enum names that map to Workshop enum domains.
///
/// `Wait.IGNORE_CONDITION` is exercised by the corpus (implicit `wait`
/// default); the remaining entries appear in the corpus `.opy` sources.
/// Additional OverPy enum spellings outside this table fail explicitly.
const KNOWN_ENUMS: &[(&str, &[&str])] = &[
    ("Beam", &["GOOD", "GRAPPLE"]),
    (
        "Color",
        &[
            "YELLOW", "WHITE", "RED", "ORANGE", "GREEN", "BLUE", "BLACK", "PURPLE", "CYAN", "TEAM",
            "AQUA", "MAGENTA", "SKY", "VIOLET", "ROSE",
        ],
    ),
    (
        "DynamicEffect",
        &[
            "BAD_EXPLOSION",
            "GOOD_EXPLOSION",
            "SPARKLES",
            "RING_EXPLOSION",
            "GOOD_AURA",
            "BAD_AURA",
            "ENERGY_SOUND",
            "GOOD_PICKUP_SOUND",
            "BAD_PICKUP_SOUND",
            "GOOD_PICKUP_EFFECT",
            "BAD_PICKUP_EFFECT",
            "BUFF_SOUND",
            "DEBUFF_SOUND",
            "BUFF_IMPACT_SOUND",
            "DEBUFF_IMPACT_SOUND",
            "REFRESH_SOUND",
            "DISCORD_SOUND",
            "AUDIBLE_BEEP",
            "COLLISION_SOUND",
            "SMALL_PICKUP_SOUND",
            "LARGE_PICKUP_SOUND",
            "SMALL_PICKUP_EFFECT",
            "LARGE_PICKUP_EFFECT",
        ],
    ),
    (
        "EffectReeval",
        &["VISIBILITY", "COLOR", "VISIBILITY_AND_COLOR"],
    ),
    ("Wait", &["IGNORE_CONDITION"]),
];

/// The lowerer's symbol context, built from the CST declarations.
struct Lowerer {
    globals: HashSet<String>,
    players: HashSet<String>,
    subroutines: HashSet<String>,
    macros: HashSet<String>,
    enums: HashMap<String, Vec<String>>,
    errors: Vec<FrontendError>,
}

/// Lower a parsed program into the Opy HIR contract.
pub fn lower(
    program: &cst::Program,
    files: Vec<SourceFile>,
    defines: Vec<Define>,
) -> FrontendResult<HirProgram> {
    let mut lowerer = Lowerer {
        globals: HashSet::new(),
        players: HashSet::new(),
        subroutines: HashSet::new(),
        macros: HashSet::new(),
        enums: HashMap::new(),
        errors: Vec::new(),
    };
    lowerer.collect_symbols(program);

    let mut declarations = Vec::new();
    for decl in &program.declarations {
        match decl {
            Decl::GlobalVariable {
                name,
                index,
                span,
                name_span,
                initializer,
            } => {
                declarations.push(Declaration::GlobalVariable {
                    name: name.clone(),
                    index: *index,
                    span: Some(span.into()),
                    name_span: Some(name_span.into()),
                    initializer: lowerer.initializer(initializer.as_ref()),
                });
            }
            Decl::PlayerVariable {
                name,
                index,
                span,
                name_span,
                initializer,
            } => {
                declarations.push(Declaration::PlayerVariable {
                    name: name.clone(),
                    index: *index,
                    span: Some(span.into()),
                    name_span: Some(name_span.into()),
                    initializer: lowerer.initializer(initializer.as_ref()),
                });
            }
            Decl::Subroutine {
                name,
                span,
                name_span,
            } => {
                declarations.push(Declaration::Subroutine {
                    name: name.clone(),
                    index: None,
                    span: Some(span.into()),
                    name_span: Some(name_span.into()),
                });
            }
            Decl::Enum { .. } => {
                // Custom enums fold to numeric constants at use sites and
                // produce no HIR declaration (reference behavior).
            }
            Decl::Macro {
                name,
                args,
                body,
                span,
            } => {
                let lowered_body = lowerer.lower_macro_body(body, args);
                declarations.push(Declaration::Macro {
                    name: name.clone(),
                    args: args.clone(),
                    span: Some(span.into()),
                    body: lowered_body,
                });
            }
        }
    }

    let mut rules = Vec::new();
    for entry in &program.rules {
        match entry {
            CstRuleEntry::Rule(rule) => rules.push(RuleEntry::Rule(lowerer.lower_rule(rule))),
            CstRuleEntry::SubroutineDef {
                name,
                span,
                name_span,
                body,
            } => {
                rules.push(RuleEntry::SubroutineDef {
                    kind: "subroutineDef".to_string(),
                    name: name.clone(),
                    span: Some(span.into()),
                    name_span: Some(name_span.into()),
                    body: lowerer.lower_block(body, &[]),
                });
            }
        }
    }

    if !lowerer.errors.is_empty() {
        return Err(lowerer.errors.swap_remove(0));
    }

    Ok(HirProgram {
        protocol: Protocol {
            name: PROTOCOL_NAME.to_string(),
            version: PROTOCOL_VERSION.to_string(),
        },
        generator: Generator {
            name: crate::FRONTEND_NAME.to_string(),
            version: crate::FRONTEND_VERSION.to_string(),
            frontend: "wright-native".to_string(),
        },
        files,
        defines,
        declarations,
        rules,
        settings: program.settings.as_ref().map(lower_settings),
    })
}

/// Map a parsed CST settings block onto the protocol settings tree (#86).
fn lower_settings(settings: &cst::Settings) -> HirSettings {
    HirSettings {
        span: Some(settings.span.into()),
        children: settings.children.iter().map(lower_settings_node).collect(),
    }
}

fn lower_settings_node(node: &cst::SettingsNode) -> HirSettingsNode {
    match node {
        cst::SettingsNode::Group {
            name,
            children,
            span,
        } => HirSettingsNode::Group {
            name: name.clone(),
            children: children.iter().map(lower_settings_node).collect(),
            span: Some((*span).into()),
        },
        cst::SettingsNode::Number { name, value, span } => HirSettingsNode::Number {
            name: name.clone(),
            value: *value,
            span: Some((*span).into()),
        },
        cst::SettingsNode::Bool { name, value, span } => HirSettingsNode::Bool {
            name: name.clone(),
            value: *value,
            span: Some((*span).into()),
        },
        cst::SettingsNode::String { name, value, span } => HirSettingsNode::String {
            name: name.clone(),
            value: value.clone(),
            span: Some((*span).into()),
        },
        cst::SettingsNode::List {
            name,
            elements,
            span,
        } => HirSettingsNode::List {
            name: name.clone(),
            elements: elements
                .iter()
                .map(|element| wright_core::hir::types::SettingsListElement {
                    value: element.value.clone(),
                    span: Some(element.span.into()),
                })
                .collect(),
            span: Some((*span).into()),
        },
    }
}

impl Lowerer {
    fn collect_symbols(&mut self, program: &cst::Program) {
        for decl in &program.declarations {
            match decl {
                Decl::GlobalVariable { name, .. } => {
                    self.globals.insert(name.clone());
                }
                Decl::PlayerVariable { name, .. } => {
                    self.players.insert(name.clone());
                }
                Decl::Subroutine { name, .. } => {
                    self.subroutines.insert(name.clone());
                }
                Decl::Enum { name, members, .. } => {
                    self.enums.insert(
                        name.clone(),
                        members.iter().map(|(member, _)| member.clone()).collect(),
                    );
                }
                Decl::Macro { name, .. } => {
                    self.macros.insert(name.clone());
                }
            }
        }
    }

    /// A declaration initializer: literal-number initializers are dropped
    /// (reference adapter behavior; non-trivial initializers are kept).
    fn initializer(&mut self, initializer: Option<&Expr>) -> Option<Box<HirExpr>> {
        let initializer = initializer?;
        let lowered = self.lower_expr(initializer, &[]);
        match &lowered {
            HirExpr::Number { .. } => None,
            other => Some(Box::new(other.clone())),
        }
    }

    fn lower_rule(&mut self, rule: &cst::Rule) -> Rule {
        let conditions = rule
            .conditions
            .iter()
            .map(|condition| self.lower_expr(condition, &[]))
            .collect();
        let actions = self.lower_block(&rule.actions, &[]);
        Rule {
            name: rule.name.clone(),
            span: Some(rule.span.into()),
            name_span: Some(rule.name_span.into()),
            disabled: rule.disabled,
            event: Event {
                name: rule.event.name.clone(),
                args: rule
                    .event
                    .args
                    .iter()
                    .map(|arg| self.lower_expr(arg, &[]))
                    .collect(),
                span: Some(rule.event.span.into()),
            },
            conditions,
            actions,
        }
    }

    /// Lower a statement block; `macro_params` names resolve to `MacroParam`.
    fn lower_block(&mut self, stmts: &[Stmt], macro_params: &[String]) -> Vec<HirStmt> {
        stmts
            .iter()
            .map(|stmt| self.lower_stmt(stmt, macro_params))
            .collect()
    }

    fn lower_stmt(&mut self, stmt: &Stmt, macro_params: &[String]) -> HirStmt {
        match stmt {
            Stmt::Expr { expr, span } => {
                // A bare call of a declared subroutine becomes
                // `CallSubroutine` (reference behavior).
                if let Expr::Call { name, args, .. } = expr {
                    if self.subroutines.contains(name) && args.is_empty() {
                        return HirStmt::CallSubroutine {
                            name: name.clone(),
                            span: Some(span.into()),
                        };
                    }
                }
                HirStmt::Expr {
                    expr: Box::new(self.lower_expr(expr, macro_params)),
                    span: Some(span.into()),
                }
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => HirStmt::Assign {
                target: Box::new(self.lower_expr(target, macro_params)),
                value: Box::new(self.lower_expr(value, macro_params)),
                span: Some(span.into()),
            },
            Stmt::If {
                branches,
                r#else,
                span,
            } => HirStmt::If {
                branches: branches
                    .iter()
                    .map(|branch| IfBranch {
                        condition: Box::new(self.lower_expr(&branch.condition, macro_params)),
                        body: self.lower_block(&branch.body, macro_params),
                    })
                    .collect(),
                r#else: r#else
                    .as_ref()
                    .map(|body| self.lower_block(body, macro_params)),
                span: Some(span.into()),
            },
            Stmt::For {
                variable,
                iterable,
                body,
                span,
            } => HirStmt::For {
                variable: Box::new(self.lower_expr(variable, macro_params)),
                iterable: Box::new(self.lower_expr(iterable, macro_params)),
                body: self.lower_block(body, macro_params),
                span: Some(span.into()),
            },
            Stmt::While {
                condition,
                body,
                span,
            } => HirStmt::While {
                condition: Box::new(self.lower_expr(condition, macro_params)),
                body: self.lower_block(body, macro_params),
                span: Some(span.into()),
            },
            Stmt::Pass { span } => HirStmt::Pass {
                span: Some(span.into()),
            },
        }
    }

    fn lower_macro_body(&mut self, body: &[Stmt], params: &[String]) -> Vec<HirStmt> {
        self.lower_block(body, params)
    }

    fn lower_expr(&mut self, expr: &Expr, macro_params: &[String]) -> HirExpr {
        match expr {
            Expr::Number { value, text, span } => HirExpr::Number {
                value: *value,
                text: text.clone(),
                span: Some(span.into()),
            },
            Expr::String { value, span } => HirExpr::String {
                value: value.clone(),
                span: Some(span.into()),
            },
            Expr::Bool { value, span } => HirExpr::Bool {
                value: *value,
                span: Some(span.into()),
            },
            Expr::Null { span } => HirExpr::Null {
                span: Some(span.into()),
            },
            Expr::Array { elements, span } => HirExpr::Array {
                elements: elements
                    .iter()
                    .map(|element| self.lower_expr(element, macro_params))
                    .collect(),
                span: Some(span.into()),
            },
            Expr::Name { name, span } => self.lower_name(name, *span, macro_params),
            Expr::Member {
                receiver,
                member,
                span,
            } => self.lower_member(receiver, member, *span, macro_params),
            Expr::Index { array, index, span } => HirExpr::Index {
                array: Box::new(self.lower_expr(array, macro_params)),
                index: Box::new(self.lower_expr(index, macro_params)),
                span: Some(span.into()),
            },
            Expr::Call { name, args, span } => self.lower_call(name, args, *span, macro_params),
            Expr::ReceiverCall {
                receiver,
                name,
                args,
                span,
            } => self.lower_receiver_call(receiver, name, args, *span, macro_params),
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => HirExpr::Binary {
                op: op.clone(),
                left: Box::new(self.lower_expr(left, macro_params)),
                right: Box::new(self.lower_expr(right, macro_params)),
                span: Some(span.into()),
            },
            Expr::Unary { op, operand, span } => HirExpr::Unary {
                op: op.clone(),
                operand: Box::new(self.lower_expr(operand, macro_params)),
                span: Some(span.into()),
            },
        }
    }

    fn lower_name(&mut self, name: &str, span: Span, macro_params: &[String]) -> HirExpr {
        if macro_params.iter().any(|param| param == name) {
            return HirExpr::MacroParam {
                name: name.to_string(),
                span: Some(span.into()),
            };
        }
        match name {
            "eventPlayer" => HirExpr::EventPlayer {
                span: Some(span.into()),
            },
            _ if self.globals.contains(name) => HirExpr::GlobalVar {
                name: name.to_string(),
                span: Some(span.into()),
            },
            _ if self.players.contains(name) => HirExpr::PlayerVar {
                player: Box::new(HirExpr::EventPlayer { span: None }),
                name: name.to_string(),
                span: Some(span.into()),
            },
            _ if self.enums.contains_key(name) => {
                self.error_at(
                    "enum-type-without-member",
                    format!("enum type '{name}' must be used with a member (e.g. {name}.MEMBER)"),
                    span,
                );
                HirExpr::Null { span: None }
            }
            _ => {
                self.error_at(
                    "unknown-identifier",
                    format!("unknown identifier '{name}'"),
                    span,
                );
                HirExpr::Null { span: None }
            }
        }
    }

    fn lower_member(
        &mut self,
        receiver: &Expr,
        member: &str,
        span: Span,
        _macro_params: &[String],
    ) -> HirExpr {
        if let Expr::Name { name, .. } = receiver {
            // Custom enum member: folds to its numeric constant.
            if let Some(members) = self.enums.get(name) {
                return match members.iter().position(|candidate| candidate == member) {
                    Some(index) => HirExpr::Number {
                        value: index as f64,
                        text: index.to_string(),
                        span: Some(span.into()),
                    },
                    None => {
                        self.error_at(
                            "unknown-enum-member",
                            format!("enum '{name}' has no member '{member}'"),
                            span,
                        );
                        HirExpr::Null { span: None }
                    }
                };
            }
            // Builtin Workshop enum.
            if let Some(members) = KNOWN_ENUMS.iter().find(|(domain, _)| *domain == name) {
                if members.1.contains(&member) {
                    return HirExpr::Enum {
                        value_type: name.clone(),
                        value: member.to_string(),
                        span: Some(span.into()),
                    };
                }
                self.error_at(
                    "unknown-enum-member",
                    format!("enum '{name}' has no member '{member}'"),
                    span,
                );
                return HirExpr::Null { span: None };
            }
            // Event-player member: a player-variable reference.
            if name == "eventPlayer" {
                return HirExpr::PlayerVar {
                    player: Box::new(HirExpr::EventPlayer { span: None }),
                    name: member.to_string(),
                    span: Some(span.into()),
                };
            }
            // A module member used without a call (`random.uniform` alone).
            if name == "random" {
                self.error_at(
                    "unsupported-member",
                    format!("module member '{name}.{member}' must be called"),
                    span,
                );
                return HirExpr::Null { span: None };
            }
        }
        self.error_at(
            "unsupported-member",
            "unsupported member access on this expression".to_string(),
            span,
        );
        HirExpr::Null { span: None }
    }

    fn lower_call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
        macro_params: &[String],
    ) -> HirExpr {
        match name {
            "vect" => {
                if args.len() != 3 {
                    self.error_at(
                        "vect-arity",
                        format!(
                            "vect() expects 3 arguments (x, y, z) but got {}",
                            args.len()
                        ),
                        span,
                    );
                    return HirExpr::Null { span: None };
                }
                HirExpr::Vector {
                    x: Box::new(self.lower_expr(&args[0], macro_params)),
                    y: Box::new(self.lower_expr(&args[1], macro_params)),
                    z: Box::new(self.lower_expr(&args[2], macro_params)),
                    span: Some(span.into()),
                }
            }
            "wait" => {
                let lowered: Vec<HirExpr> = args
                    .iter()
                    .map(|arg| self.lower_expr(arg, macro_params))
                    .collect();
                let mut result = lowered;
                match result.len() {
                    0 => {
                        // Reference default: wait(0.016, Wait.IGNORE_CONDITION).
                        result.push(HirExpr::Number {
                            value: 0.016,
                            text: "0.016".to_string(),
                            span: Some(span.into()),
                        });
                        result.push(HirExpr::Enum {
                            value_type: "Wait".to_string(),
                            value: "IGNORE_CONDITION".to_string(),
                            span: Some(span.into()),
                        });
                    }
                    1 => {
                        result.push(HirExpr::Enum {
                            value_type: "Wait".to_string(),
                            value: "IGNORE_CONDITION".to_string(),
                            span: Some(span.into()),
                        });
                    }
                    _ => {}
                }
                HirExpr::Call {
                    name: name.to_string(),
                    args: result,
                    span: Some(span.into()),
                }
            }
            _ if self.macros.contains(name) => {
                // A declared `macro` invocation is recorded as a macroCall.
                HirExpr::MacroCall {
                    name: name.to_string(),
                    args: args
                        .iter()
                        .map(|arg| self.lower_expr(arg, macro_params))
                        .collect(),
                    span: Some(span.into()),
                }
            }
            _ => HirExpr::Call {
                name: name.to_string(),
                args: args
                    .iter()
                    .map(|arg| self.lower_expr(arg, macro_params))
                    .collect(),
                span: Some(span.into()),
            },
        }
    }

    fn lower_receiver_call(
        &mut self,
        receiver: &Expr,
        name: &str,
        args: &[Expr],
        span: Span,
        macro_params: &[String],
    ) -> HirExpr {
        let lowered_args: Vec<HirExpr> = args
            .iter()
            .map(|arg| self.lower_expr(arg, macro_params))
            .collect();
        match receiver {
            // `random.uniform(...)` → dotted call name.
            Expr::Name { name: root, .. } if root == "random" => HirExpr::Call {
                name: format!("random.{name}"),
                args: lowered_args,
                span: Some(span.into()),
            },
            // `"text".format(...)` → format node.
            Expr::String { value, .. } if name == "format" => HirExpr::Format {
                text: value.clone(),
                args: lowered_args,
                span: Some(span.into()),
            },
            // `eventPlayer.hasSpawned()` → receiver call on the event player.
            Expr::Name { name: root, .. } if root == "eventPlayer" => HirExpr::ReceiverCall {
                receiver: Box::new(HirExpr::EventPlayer { span: None }),
                name: name.to_string(),
                args: lowered_args,
                span: Some(span.into()),
            },
            // Any other receiver: resolve it and keep the receiver call.
            other => HirExpr::ReceiverCall {
                receiver: Box::new(self.lower_expr(other, macro_params)),
                name: name.to_string(),
                args: lowered_args,
                span: Some(span.into()),
            },
        }
    }

    fn error_at(&mut self, code: &str, message: String, span: Span) {
        self.errors.push(FrontendError::at(code, message, span));
    }
}

impl From<Span> for HirSpan {
    fn from(span: Span) -> HirSpan {
        HirSpan {
            file: span.file,
            start: Position {
                line: span.start.line,
                col: span.start.col,
            },
            end: Position {
                line: span.end.line,
                col: span.end.col,
            },
        }
    }
}

impl From<&Span> for HirSpan {
    fn from(span: &Span) -> HirSpan {
        (*span).into()
    }
}
