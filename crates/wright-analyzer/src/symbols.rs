use std::collections::HashSet;

pub use workshop_rs::ids::Id;
use workshop_rs::source::Span;
use workshop_rs::{Action, Event, Program, Value};

pub type RuleId = usize;
pub type ActionId = usize;
pub type ValueId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    GlobalVariable,
    PlayerVariable,
    Subroutine,
    Rule,
}

impl SymbolKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GlobalVariable => "globalVariable",
            Self::PlayerVariable => "playerVariable",
            Self::Subroutine => "subroutine",
            Self::Rule => "rule",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    Declaration,
    Definition,
    Read,
    Write,
    Call,
}

impl ReferenceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Declaration => "declaration",
            Self::Definition => "definition",
            Self::Read => "read",
            Self::Write => "write",
            Self::Call => "call",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub id: SymbolId,
    pub kind: SymbolKind,
    pub name: String,
    pub span: Option<Span>,
    pub occurrence: Option<Span>,
    pub rule: Option<RuleId>,
}

pub type SymbolId = Id<Symbol>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub symbol: SymbolId,
    pub kind: ReferenceKind,
    pub span: Option<Span>,
    pub occurrence: Option<Span>,
    pub rule: Option<RuleId>,
    pub action: Option<ActionId>,
    pub value: Option<ValueId>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageSummary {
    pub reads: u32,
    pub writes: u32,
    pub calls: u32,
    pub rules: u32,
}

#[derive(Debug, Clone)]
pub struct SemanticIndex {
    symbols: Vec<Symbol>,
    references: Vec<Reference>,
}

impl SemanticIndex {
    pub fn build(program: &Program) -> Self {
        let mut symbols = Vec::new();
        let mut register =
            |name: &str, kind, prefix: Option<&str>, rule: Option<usize>, span: Option<Span>| {
                let id = SymbolId::from_index(symbols.len());
                let occ = prefix
                    .and_then(|p| declaration_span(program, &[p], name))
                    .or(span);
                symbols.push(Symbol {
                    id,
                    kind,
                    name: name.to_string(),
                    span: occ,
                    occurrence: occ,
                    rule,
                });
            };
        for (vars, kind, prefix) in [
            (
                &program.global_variables,
                SymbolKind::GlobalVariable,
                "globalvar ",
            ),
            (
                &program.player_variables,
                SymbolKind::PlayerVariable,
                "playervar ",
            ),
        ] {
            for v in vars {
                register(&v.name, kind, Some(prefix), None, None);
            }
        }
        for s in &program.subroutines {
            register(
                &s.name,
                SymbolKind::Subroutine,
                Some("subroutine "),
                None,
                None,
            );
        }
        for (r, d) in program.rules.iter().enumerate() {
            register(
                &d.name,
                SymbolKind::Rule,
                None,
                Some(r),
                program.rule_span(r),
            );
        }
        let mut index = Self {
            symbols,
            references: Vec::new(),
        };
        for symbol in index.symbols.clone() {
            if let Some(span) = symbol.occurrence {
                index.push(
                    symbol.id,
                    ReferenceKind::Declaration,
                    symbol.rule,
                    None,
                    None,
                    Some(span),
                );
            }
        }
        for (rule, data) in program.rules.iter().enumerate() {
            index.walk_event(&data.event, rule, program);
            for (condition, value) in data.conditions.iter().enumerate() {
                index.walk_value(
                    &value.value,
                    rule,
                    None,
                    Some(condition),
                    program.condition_span(rule, condition),
                    program,
                );
            }
            for (action, value) in data.actions.iter().enumerate() {
                index.walk_action(value, rule, action, program);
            }
        }
        index
    }

    pub fn symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.iter()
    }

    pub fn symbol(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(id.index())
    }

    pub fn references(&self, symbol: SymbolId) -> Vec<&Reference> {
        self.references
            .iter()
            .filter(|reference| reference.symbol == symbol)
            .collect()
    }

    pub fn usage(&self, symbol: SymbolId) -> UsageSummary {
        let mut usage = UsageSummary::default();
        let mut rules = HashSet::new();
        for reference in self.references(symbol) {
            match reference.kind {
                ReferenceKind::Read => usage.reads += 1,
                ReferenceKind::Write => usage.writes += 1,
                ReferenceKind::Call => usage.calls += 1,
                _ => {}
            }
            if let Some(rule) = reference.rule {
                rules.insert(rule);
            }
        }
        usage.rules = rules.len() as u32;
        usage
    }

    fn push(
        &mut self,
        symbol: SymbolId,
        kind: ReferenceKind,
        rule: Option<RuleId>,
        action: Option<ActionId>,
        value: Option<ValueId>,
        span: Option<Span>,
    ) {
        self.references.push(Reference {
            symbol,
            kind,
            span,
            occurrence: span,
            rule,
            action,
            value,
        });
    }

    fn symbol_by(&self, kind: SymbolKind, name: &str) -> Option<SymbolId> {
        self.symbols
            .iter()
            .find(|s| s.kind == kind && s.name == name)
            .map(|s| s.id)
    }

    fn walk_event(&mut self, event: &Event, rule: RuleId, program: &Program) {
        if let Event::Subroutine(name) = event {
            if let Some(symbol) = self.symbol_by(SymbolKind::Subroutine, name) {
                let rule_span = program.rule_span(rule);
                self.push(
                    symbol,
                    ReferenceKind::Definition,
                    Some(rule),
                    None,
                    None,
                    text_occurrence(program, rule_span, name, false).or(rule_span),
                );
            }
        }
    }

    fn write_var(
        &mut self,
        kind: SymbolKind,
        var: &str,
        (rule, act, span): (RuleId, ActionId, Option<Span>),
        prog: &Program,
        is_modify: bool,
        before_eq: bool,
    ) {
        if let Some(sym) = self.symbol_by(kind, var) {
            let occ = text_occurrence(prog, span, var, before_eq).or(span);
            self.push(sym, ReferenceKind::Write, Some(rule), Some(act), None, occ);
            if is_modify {
                self.push(sym, ReferenceKind::Read, Some(rule), Some(act), None, span);
            }
        }
    }

    fn walk_args(&mut self, args: &[(usize, &Value)], rule: RuleId, act: ActionId, prog: &Program) {
        for (i, v) in args {
            self.walk_value(
                v,
                rule,
                Some(act),
                None,
                prog.action_argument_span(rule, act, *i),
                prog,
            );
        }
    }

    fn read_var(
        &mut self,
        kind: SymbolKind,
        name: &str,
        (rule, act, val): (RuleId, Option<ActionId>, Option<ValueId>),
        span: Option<Span>,
        prog: &Program,
    ) {
        if let Some(sym) = self.symbol_by(kind, name) {
            let occ = text_occurrence(prog, span, name, false).or(span);
            self.push(sym, ReferenceKind::Read, Some(rule), act, val, occ);
        }
    }

    fn walk_action(
        &mut self,
        action: &Action,
        rule: RuleId,
        action_id: ActionId,
        program: &Program,
    ) {
        let span = program.action_span(rule, action_id);
        match action {
            Action::SetGlobalVariable { variable, value }
            | Action::ModifyGlobalVariable {
                variable, value, ..
            } => {
                let is_mod = matches!(action, Action::ModifyGlobalVariable { .. });
                self.write_var(
                    SymbolKind::GlobalVariable,
                    variable,
                    (rule, action_id, span),
                    program,
                    is_mod,
                    true,
                );
                self.walk_args(&[(1, value)], rule, action_id, program);
            }
            Action::SetPlayerVariable {
                player,
                variable,
                value,
            }
            | Action::ModifyPlayerVariable {
                player,
                variable,
                value,
                ..
            } => {
                let is_mod = matches!(action, Action::ModifyPlayerVariable { .. });
                self.write_var(
                    SymbolKind::PlayerVariable,
                    variable,
                    (rule, action_id, span),
                    program,
                    is_mod,
                    true,
                );
                self.walk_args(&[(0, player), (2, value)], rule, action_id, program);
            }
            Action::AssignMember { target, value, .. } => {
                self.walk_args(&[(0, target), (1, value)], rule, action_id, program);
            }
            Action::CallSubroutine { subroutine } => {
                if let Some(symbol) = self.symbol_by(SymbolKind::Subroutine, subroutine) {
                    self.push(
                        symbol,
                        ReferenceKind::Call,
                        Some(rule),
                        Some(action_id),
                        None,
                        span,
                    );
                }
            }
            Action::ForGlobalVariable {
                variable,
                start,
                stop,
                step,
            } => {
                self.write_var(
                    SymbolKind::GlobalVariable,
                    variable,
                    (rule, action_id, span),
                    program,
                    false,
                    false,
                );
                self.walk_args(
                    &[(1, start), (2, stop), (3, step)],
                    rule,
                    action_id,
                    program,
                );
            }
            Action::ForPlayerVariable {
                player,
                variable,
                start,
                stop,
                step,
            } => {
                self.write_var(
                    SymbolKind::PlayerVariable,
                    variable,
                    (rule, action_id, span),
                    program,
                    false,
                    false,
                );
                self.walk_args(
                    &[(0, player), (1, start), (2, stop), (3, step)],
                    rule,
                    action_id,
                    program,
                );
            }
            Action::If { condition }
            | Action::ElseIf { condition }
            | Action::While { condition } => {
                self.walk_args(&[(0, condition)], rule, action_id, program);
            }
            Action::Call { args, .. } => {
                for (arg, value) in args.iter().enumerate() {
                    self.walk_value(
                        value,
                        rule,
                        Some(action_id),
                        None,
                        program.action_argument_span(rule, action_id, arg),
                        program,
                    );
                }
            }
            Action::Disabled { action } => self.walk_action(action, rule, action_id, program),
            Action::Else | Action::End => {}
        }
    }

    fn walk_value(
        &mut self,
        value: &Value,
        rule: RuleId,
        action: Option<ActionId>,
        value_id: Option<ValueId>,
        span: Option<Span>,
        program: &Program,
    ) {
        match value {
            Value::GlobalVariable(name) => {
                self.read_var(
                    SymbolKind::GlobalVariable,
                    name,
                    (rule, action, value_id),
                    span,
                    program,
                );
            }
            Value::PlayerVariable { player, variable } => {
                self.read_var(
                    SymbolKind::PlayerVariable,
                    variable,
                    (rule, action, value_id),
                    span,
                    program,
                );
                self.walk_value(player, rule, action, value_id, span, program);
            }
            Value::Array(values) | Value::Call { args: values, .. } => {
                for value in values {
                    self.walk_value(value, rule, action, value_id, span, program);
                }
            }
            Value::Vector { x, y, z } => {
                for comp in [x, y, z] {
                    self.walk_value(comp, rule, action, value_id, span, program);
                }
            }
            _ => {}
        }
    }
}

fn declaration_span(program: &Program, prefixes: &[&str], name: &str) -> Option<Span> {
    for file_index in 0..64 {
        let file = workshop_rs::source::FileId::from_index(file_index);
        let Some(source) = program.source(file) else {
            continue;
        };
        for (line_index, line) in source.text().lines().enumerate() {
            for prefix in prefixes {
                if let Some(rest) = line.strip_prefix(prefix) {
                    if rest
                        .split_whitespace()
                        .next()
                        .is_some_and(|f| f.trim_matches('"') == name)
                    {
                        let start = prefix.chars().count() as u32 + 1;
                        let line_num = line_index as u32 + 1;
                        return Some(Span::new(
                            file,
                            workshop_rs::source::Position::new(line_num, start),
                            workshop_rs::source::Position::new(
                                line_num,
                                start + name.chars().count() as u32,
                            ),
                        ));
                    }
                }
            }
        }
    }
    None
}

fn text_occurrence(
    program: &Program,
    span: Option<Span>,
    name: &str,
    before_eq: bool,
) -> Option<Span> {
    let span = span?;
    let source = program.source(span.file)?.text();
    let name_chars: Vec<char> = name.chars().collect();
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    for lno in span.start.line..=span.end.line {
        let line = source.lines().nth(lno.saturating_sub(1) as usize)?;
        let chars: Vec<char> = line.chars().collect();
        let lower = if lno == span.start.line {
            span.start.col.saturating_sub(1) as usize
        } else {
            0
        };
        let mut upper = if lno == span.end.line {
            span.end.col.saturating_sub(1) as usize
        } else {
            chars.len()
        };
        if before_eq {
            if let Some(op) = line.find('=') {
                upper = upper.min(op);
            }
        }
        for start in lower.min(chars.len())..=upper.min(chars.len()) {
            let end = start + name_chars.len();
            if end <= upper && chars.get(start..end) == Some(name_chars.as_slice()) {
                let before = start
                    .checked_sub(1)
                    .and_then(|i| chars.get(i))
                    .copied()
                    .is_some_and(is_ident);
                let after = chars.get(end).copied().is_some_and(is_ident);
                if !before && !after {
                    return Some(Span::new(
                        span.file,
                        workshop_rs::source::Position::new(lno, start as u32 + 1),
                        workshop_rs::source::Position::new(lno, end as u32 + 1),
                    ));
                }
            }
        }
    }
    None
}
