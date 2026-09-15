use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::{Value as JsonValue, json};
use workshop_rs::source::Span;
use workshop_rs::{Action, Event, ModifyOp, Program, Rule, Value};
use wright_ir::ids::Id;

use crate::analysis::{Boundedness, EvidenceClass, Severity};
use crate::registry::LintConfig;
use crate::service::{ErrorInfo, Origin, Request, Response};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    Declaration,
    Definition,
    Read,
    Write,
    Call,
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
        for variable in &program.global_variables {
            let id = SymbolId::from_index(symbols.len());
            let occurrence = declaration_span(program, &["globalvar "], &variable.name);
            symbols.push(Symbol {
                id,
                kind: SymbolKind::GlobalVariable,
                name: variable.name.clone(),
                span: occurrence,
                occurrence,
                rule: None,
            });
        }
        for variable in &program.player_variables {
            let id = SymbolId::from_index(symbols.len());
            let occurrence = declaration_span(program, &["playervar "], &variable.name);
            symbols.push(Symbol {
                id,
                kind: SymbolKind::PlayerVariable,
                name: variable.name.clone(),
                span: occurrence,
                occurrence,
                rule: None,
            });
        }
        for subroutine in &program.subroutines {
            let id = SymbolId::from_index(symbols.len());
            let occurrence = declaration_span(program, &["subroutine "], &subroutine.name);
            symbols.push(Symbol {
                id,
                kind: SymbolKind::Subroutine,
                name: subroutine.name.clone(),
                span: occurrence,
                occurrence,
                rule: None,
            });
        }
        for (rule, data) in program.rules.iter().enumerate() {
            let id = SymbolId::from_index(symbols.len());
            symbols.push(Symbol {
                id,
                kind: SymbolKind::Rule,
                name: data.name.clone(),
                span: program.rule_span(rule),
                occurrence: program.rule_span(rule),
                rule: Some(rule),
            });
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

    pub fn build_with_sources(
        program: &Program,
        sources: &[(workshop_rs::source::FileId, String)],
    ) -> Self {
        let mut index = Self::build(program);
        for symbol in &mut index.symbols {
            let prefixes = match symbol.kind {
                SymbolKind::GlobalVariable => &["globalvar "][..],
                SymbolKind::PlayerVariable => &["playervar "][..],
                SymbolKind::Subroutine => &["subroutine "][..],
                SymbolKind::Rule => &[][..],
            };
            if let Some(span) = declaration_span_in_sources(sources, prefixes, &symbol.name) {
                symbol.span = Some(declaration_line_span(sources, span));
                symbol.occurrence = Some(span);
            }
        }
        for symbol in index.symbols.clone() {
            if let Some(span) = symbol.occurrence {
                if !index.references.iter().any(|reference| {
                    reference.symbol == symbol.id && reference.kind == ReferenceKind::Declaration
                }) {
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
        }
        let symbols = index.symbols.clone();
        let mut read_occurrences = HashMap::new();
        for reference in &mut index.references {
            let symbol = symbols
                .get(reference.symbol.index())
                .cloned()
                .expect("reference symbol exists");
            let span = match reference.kind {
                ReferenceKind::Declaration => symbol.occurrence,
                ReferenceKind::Definition => reference
                    .rule
                    .and_then(|rule| program.rule_span(rule))
                    .and_then(|span| occurrence_in_sources(sources, span, &symbol.name, false, 0)),
                ReferenceKind::Write | ReferenceKind::Call => reference
                    .rule
                    .and_then(|rule| {
                        reference
                            .action
                            .and_then(|action| program.action_span(rule, action))
                    })
                    .and_then(|span| occurrence_in_sources(sources, span, &symbol.name, true, 0)),
                ReferenceKind::Read => {
                    let key = (
                        reference.symbol,
                        reference.rule,
                        reference.action,
                        reference.value,
                    );
                    let ordinal = read_occurrences.entry(key).or_insert(0);
                    let action_span = reference.rule.and_then(|rule| {
                        reference
                            .action
                            .and_then(|action| program.action_span(rule, action))
                    });
                    let implicit_modify = reference.value.is_none()
                        && reference.span.is_some()
                        && reference.span == action_span;
                    let current = if implicit_modify { 1 } else { *ordinal };
                    *ordinal += 1;
                    reference
                        .span
                        .or_else(|| {
                            action_span.or_else(|| {
                                reference.rule.and_then(|rule| {
                                    reference
                                        .value
                                        .and_then(|value| program.condition_span(rule, value))
                                })
                            })
                        })
                        .and_then(|span| {
                            occurrence_in_sources(sources, span, &symbol.name, false, current)
                        })
                }
            };
            reference.span = span;
            reference.occurrence = span;
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
    fn global(&self, name: &str) -> Option<SymbolId> {
        self.symbols
            .iter()
            .find(|s| s.kind == SymbolKind::GlobalVariable && s.name == name)
            .map(|s| s.id)
    }
    fn player(&self, name: &str) -> Option<SymbolId> {
        self.symbols
            .iter()
            .find(|s| s.kind == SymbolKind::PlayerVariable && s.name == name)
            .map(|s| s.id)
    }
    fn subroutine(&self, name: &str) -> Option<SymbolId> {
        self.symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Subroutine && s.name == name)
            .map(|s| s.id)
    }
    fn walk_event(&mut self, event: &Event, rule: RuleId, program: &Program) {
        if let Event::Subroutine(name) = event {
            if let Some(symbol) = self.subroutine(name) {
                self.push(
                    symbol,
                    ReferenceKind::Definition,
                    Some(rule),
                    None,
                    None,
                    action_occurrence(program, program.rule_span(rule), name),
                );
            }
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
                if let Some(symbol) = self.global(variable) {
                    self.push(
                        symbol,
                        ReferenceKind::Write,
                        Some(rule),
                        Some(action_id),
                        None,
                        action_occurrence(program, span, variable),
                    );
                    if matches!(action, Action::ModifyGlobalVariable { .. }) {
                        self.push(
                            symbol,
                            ReferenceKind::Read,
                            Some(rule),
                            Some(action_id),
                            None,
                            span,
                        );
                    }
                }
                self.walk_value(
                    value,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 1),
                    program,
                );
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
                if let Some(symbol) = self.player(variable) {
                    self.push(
                        symbol,
                        ReferenceKind::Write,
                        Some(rule),
                        Some(action_id),
                        None,
                        action_occurrence(program, span, variable),
                    );
                    if matches!(action, Action::ModifyPlayerVariable { .. }) {
                        self.push(
                            symbol,
                            ReferenceKind::Read,
                            Some(rule),
                            Some(action_id),
                            None,
                            span,
                        );
                    }
                }
                self.walk_value(
                    player,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 0),
                    program,
                );
                self.walk_value(
                    value,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 2),
                    program,
                );
            }
            Action::AssignMember { target, value, .. } => {
                self.walk_value(
                    target,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 0),
                    program,
                );
                self.walk_value(
                    value,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 1),
                    program,
                );
            }
            Action::CallSubroutine { subroutine } => {
                if let Some(symbol) = self.subroutine(subroutine) {
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
            Action::If { condition }
            | Action::ElseIf { condition }
            | Action::While { condition } => self.walk_value(
                condition,
                rule,
                Some(action_id),
                None,
                program.action_argument_span(rule, action_id, 0),
                program,
            ),
            Action::ForGlobalVariable {
                variable,
                start,
                stop,
                step,
            } => {
                if let Some(symbol) = self.global(variable) {
                    self.push(
                        symbol,
                        ReferenceKind::Write,
                        Some(rule),
                        Some(action_id),
                        None,
                        action_occurrence(program, span, variable),
                    );
                }
                self.walk_value(
                    start,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 1),
                    program,
                );
                self.walk_value(
                    stop,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 2),
                    program,
                );
                self.walk_value(
                    step,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 3),
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
                if let Some(symbol) = self.player(variable) {
                    self.push(
                        symbol,
                        ReferenceKind::Write,
                        Some(rule),
                        Some(action_id),
                        None,
                        action_occurrence(program, span, variable),
                    );
                }
                self.walk_value(
                    player,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 0),
                    program,
                );
                self.walk_value(
                    start,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 1),
                    program,
                );
                self.walk_value(
                    stop,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 2),
                    program,
                );
                self.walk_value(
                    step,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 3),
                    program,
                );
            }
            Action::Disabled { action } => self.walk_action(action, rule, action_id, program),
            Action::Call { args, .. } => {
                for (argument, value) in args.iter().enumerate() {
                    self.walk_value(
                        value,
                        rule,
                        Some(action_id),
                        None,
                        program.action_argument_span(rule, action_id, argument),
                        program,
                    );
                }
            }
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
                if let Some(symbol) = self.global(name) {
                    self.push(
                        symbol,
                        ReferenceKind::Read,
                        Some(rule),
                        action,
                        value_id,
                        value_occurrence(program, span, name).or(span),
                    );
                }
            }
            Value::PlayerVariable { player, variable } => {
                if let Some(symbol) = self.player(variable) {
                    self.push(
                        symbol,
                        ReferenceKind::Read,
                        Some(rule),
                        action,
                        value_id,
                        value_occurrence(program, span, variable).or(span),
                    );
                }
                self.walk_value(player, rule, action, value_id, span, program);
            }
            Value::Array(values) | Value::Call { args: values, .. } => {
                for value in values {
                    self.walk_value(value, rule, action, value_id, span, program);
                }
            }
            Value::Vector { x, y, z } => {
                self.walk_value(x, rule, action, value_id, span, program);
                self.walk_value(y, rule, action, value_id, span, program);
                self.walk_value(z, rule, action, value_id, span, program);
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
                let Some(rest) = line.strip_prefix(prefix) else {
                    continue;
                };
                let Some(found) = rest.split_whitespace().next() else {
                    continue;
                };
                let found = found.trim_matches('"');
                if found != name {
                    continue;
                }
                let start = prefix.chars().count() as u32 + 1;
                return Some(Span::new(
                    file,
                    workshop_rs::source::Position::new(line_index as u32 + 1, start),
                    workshop_rs::source::Position::new(
                        line_index as u32 + 1,
                        start + name.chars().count() as u32,
                    ),
                ));
            }
        }
    }
    None
}

fn declaration_span_in_sources(
    sources: &[(workshop_rs::source::FileId, String)],
    prefixes: &[&str],
    name: &str,
) -> Option<Span> {
    for (file, source) in sources {
        for (line_index, line) in source.lines().enumerate() {
            for prefix in prefixes {
                let Some(rest) = line.strip_prefix(prefix) else {
                    continue;
                };
                let Some(found) = rest.split_whitespace().next() else {
                    continue;
                };
                let found = found.trim_matches('"');
                if found != name {
                    continue;
                }
                let start = prefix.chars().count() as u32 + 1;
                return Some(Span::new(
                    *file,
                    workshop_rs::source::Position::new(line_index as u32 + 1, start),
                    workshop_rs::source::Position::new(
                        line_index as u32 + 1,
                        start + name.chars().count() as u32,
                    ),
                ));
            }
        }
    }
    None
}

fn declaration_line_span(
    sources: &[(workshop_rs::source::FileId, String)],
    name_span: Span,
) -> Span {
    let end_col = sources
        .iter()
        .find(|(file, _)| *file == name_span.file)
        .and_then(|(_, source)| {
            source
                .lines()
                .nth(name_span.start.line.saturating_sub(1) as usize)
                .map(|line| line.chars().count() as u32 + 1)
        })
        .unwrap_or(name_span.end.col);
    Span::new(
        name_span.file,
        workshop_rs::source::Position::new(name_span.start.line, 1),
        workshop_rs::source::Position::new(name_span.start.line, end_col),
    )
}

fn occurrence_in_sources(
    sources: &[(workshop_rs::source::FileId, String)],
    span: Span,
    name: &str,
    before_assignment: bool,
    ordinal: usize,
) -> Option<Span> {
    let source = sources
        .iter()
        .find(|(file, _)| *file == span.file)
        .map(|(_, source)| source.as_str())?;
    let name_chars: Vec<char> = name.chars().collect();
    let mut found_index = 0;
    for line_number in span.start.line..=span.end.line {
        let line = source.lines().nth(line_number.saturating_sub(1) as usize)?;
        let chars: Vec<char> = line.chars().collect();
        let lower = if line_number == span.start.line {
            span.start.col.saturating_sub(1) as usize
        } else {
            0
        };
        let mut upper = if line_number == span.end.line {
            span.end.col.saturating_sub(1) as usize
        } else {
            chars.len()
        };
        if before_assignment {
            if let Some(operator) = line.find('=') {
                upper = upper.min(operator);
            }
        }
        for start in lower.min(chars.len())..=upper.min(chars.len()) {
            let end = start.saturating_add(name_chars.len());
            if end > upper || chars.get(start..end) != Some(name_chars.as_slice()) {
                continue;
            }
            if !is_code_position(&chars, start) {
                continue;
            }
            let before = start.checked_sub(1).and_then(|index| chars.get(index));
            let after = chars.get(end);
            if before.is_some_and(|character| character.is_alphanumeric() || *character == '_')
                || after.is_some_and(|character| character.is_alphanumeric() || *character == '_')
            {
                continue;
            }
            if found_index == ordinal {
                return Some(Span::new(
                    span.file,
                    workshop_rs::source::Position::new(line_number, start as u32 + 1),
                    workshop_rs::source::Position::new(line_number, end as u32 + 1),
                ));
            }
            found_index += 1;
        }
    }
    None
}

fn is_code_position(chars: &[char], position: usize) -> bool {
    let mut quoted = false;
    let mut escaped = false;
    for character in chars.iter().take(position) {
        if *character == '#' && !quoted {
            return false;
        }
        if *character == '"' && !escaped {
            quoted = !quoted;
        }
        escaped = *character == '\\' && !escaped;
        if *character != '\\' {
            escaped = false;
        }
    }
    !quoted
}

fn action_occurrence(program: &Program, span: Option<Span>, name: &str) -> Option<Span> {
    let span = span?;
    let source = program.source(span.file)?.text();
    let name_chars: Vec<char> = name.chars().collect();
    for line_number in span.start.line..=span.end.line {
        let line = source.lines().nth(line_number.saturating_sub(1) as usize)?;
        let chars: Vec<char> = line.chars().collect();
        let lower = if line_number == span.start.line {
            span.start.col.saturating_sub(1) as usize
        } else {
            0
        };
        let mut upper = if line_number == span.end.line {
            span.end.col.saturating_sub(1) as usize
        } else {
            chars.len()
        };
        if let Some(operator) = line.find("=") {
            upper = upper.min(operator);
        }
        for start in lower.min(chars.len())..=upper.min(chars.len()) {
            let end = start.saturating_add(name_chars.len());
            if end > upper || chars.get(start..end) != Some(name_chars.as_slice()) {
                continue;
            }
            let before = start.checked_sub(1).and_then(|index| chars.get(index));
            let after = chars.get(end);
            if before.is_some_and(|character| character.is_alphanumeric() || *character == '_')
                || after.is_some_and(|character| character.is_alphanumeric() || *character == '_')
            {
                continue;
            }
            return Some(Span::new(
                span.file,
                workshop_rs::source::Position::new(line_number, start as u32 + 1),
                workshop_rs::source::Position::new(line_number, end as u32 + 1),
            ));
        }
    }
    None
}

fn value_occurrence(program: &Program, span: Option<Span>, name: &str) -> Option<Span> {
    let span = span?;
    let source = program.source(span.file)?.text();
    let name_chars: Vec<char> = name.chars().collect();
    for line_number in span.start.line..=span.end.line {
        let line = source.lines().nth(line_number.saturating_sub(1) as usize)?;
        let chars: Vec<char> = line.chars().collect();
        let lower = if line_number == span.start.line {
            span.start.col.saturating_sub(1) as usize
        } else {
            0
        };
        let upper = if line_number == span.end.line {
            span.end.col.saturating_sub(1) as usize
        } else {
            chars.len()
        };
        for start in lower.min(chars.len())..=upper.min(chars.len()) {
            let end = start.saturating_add(name_chars.len());
            if end > upper || chars.get(start..end) != Some(name_chars.as_slice()) {
                continue;
            }
            let before = start.checked_sub(1).and_then(|index| chars.get(index));
            let after = chars.get(end);
            if before.is_some_and(|character| character.is_alphanumeric() || *character == '_')
                || after.is_some_and(|character| character.is_alphanumeric() || *character == '_')
            {
                continue;
            }
            return Some(Span::new(
                span.file,
                workshop_rs::source::Position::new(line_number, start as u32 + 1),
                workshop_rs::source::Position::new(line_number, end as u32 + 1),
            ));
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
    pub rule: RuleId,
    pub action: Option<ActionId>,
    pub value: Option<ValueId>,
    pub evidence: EvidenceClass,
    pub boundedness: Option<Boundedness>,
}

pub struct SemanticService<'a> {
    program: &'a Program,
    index: SemanticIndex,
    findings: Vec<Finding>,
    origin: Origin,
    config: LintConfig,
    registry: Arc<crate::registry::LintRegistry>,
}

impl<'a> SemanticService<'a> {
    pub fn with_origin_and_config(
        program: &'a Program,
        origin: Origin,
        config: LintConfig,
    ) -> Self {
        Self::with_origin_and_config_and_registry(
            program,
            origin,
            config,
            Arc::new(crate::registry::LintRegistry::default()),
        )
    }
    pub fn with_origin_and_config_and_registry(
        program: &'a Program,
        origin: Origin,
        config: LintConfig,
        registry: Arc<crate::registry::LintRegistry>,
    ) -> Self {
        let index = SemanticIndex::build(program);
        let mut findings = analyze(program, &config);
        findings.extend(registry.run_canonical_custom(program, &config));
        Self {
            program,
            index,
            findings,
            origin,
            config,
            registry,
        }
    }
    pub fn handle(&self, request: &Request) -> Response {
        match request {
            Request::Version => Response::Ok { result: json!({"name": "wright-tool", "version": env!("CARGO_PKG_VERSION"), "capabilities": ["program", "rules", "symbols", "references", "usage", "cfg", "findings", "persistentObjects", "lintRules"]}) },
            Request::Program => Response::Ok { result: json!({"origin": self.origin, "files": file_count(self.program), "globalVariables": self.program.global_variables.len(), "playerVariables": self.program.player_variables.len(), "subroutines": self.program.subroutines.len(), "rules": self.program.rules.len(), "findings": self.findings.len()}) },
            Request::ListRules => Response::Ok { result: json!(self.program.rules.iter().enumerate().map(|(id, rule)| json!({"id": id, "name": rule.name, "span": span_json(self.program.rule_span(id))})).collect::<Vec<_>>()) },
            Request::GetRule { rule } => self.rule(*rule as usize),
            Request::ListSymbols { kind } => Response::Ok { result: json!(self.index.symbols().filter(|symbol| kind.as_deref().is_none_or(|kind| symbol_kind_name(symbol.kind) == kind)).map(symbol_json).collect::<Vec<_>>()) },
            Request::GetSymbol { symbol } => self.index.symbol(Id::from_index(*symbol as usize)).map_or_else(|| self.error("invalid-id", format!("unknown symbol {symbol}")), |symbol| Response::Ok { result: symbol_json(symbol) }),
            Request::FindReferences { symbol } => { let id = Id::from_index(*symbol as usize); if self.index.symbol(id).is_none() { self.error("invalid-id", format!("unknown symbol {symbol}")) } else { Response::Ok { result: json!(self.index.references(id).into_iter().map(|reference| json!({"kind": reference_kind_name(reference.kind), "span": span_json(reference.span), "rule": reference.rule, "action": reference.action, "value": reference.value})).collect::<Vec<_>>()) } } }
            Request::GetUsage { symbol } => { let id = Id::from_index(*symbol as usize); self.index.symbol(id).map_or_else(|| self.error("invalid-id", format!("unknown symbol {symbol}")), |data| { let usage = self.index.usage(id); Response::Ok { result: json!({"symbol": data.name, "reads": usage.reads, "writes": usage.writes, "calls": usage.calls, "rules": usage.rules}) } }) }
            Request::GetCfg { rule } => cfg_response(self.program, *rule as usize),
            Request::GetFindings => Response::Ok { result: json!(self.findings.iter().map(finding_json).collect::<Vec<_>>()) },
            Request::GetPersistentObjects => Response::Ok { result: json!(persistent_objects(self.program)) },
            Request::LintRules => Response::Ok {
                result: lint_rules(&self.registry, &self.config),
            },
        }
    }
    fn rule(&self, id: RuleId) -> Response {
        let Some(rule) = self.program.rules.get(id) else {
            return self.error("invalid-id", format!("unknown rule {id}"));
        };
        Response::Ok {
            result: json!({"id": id, "name": rule.name, "span": span_json(self.program.rule_span(id)), "disabled": rule.disabled, "event": event_name(&rule.event), "conditions": rule.conditions.len(), "actions": rule.actions.len()}),
        }
    }
    fn error(&self, code: &str, message: String) -> Response {
        Response::Error {
            error: ErrorInfo {
                code: code.to_string(),
                message,
            },
        }
    }
}

pub fn analyze(program: &Program, config: &LintConfig) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (rule_id, rule) in program.rules.iter().enumerate() {
        if rule.disabled {
            continue;
        }
        for (action_id, action) in rule.actions.iter().enumerate() {
            let Some((start, end)) = loop_body(rule, action_id) else {
                continue;
            };
            let body = &rule.actions[start..end];
            if config.is_enabled("min-wait-loop") && body.iter().any(|action| is_wait(action, true))
            {
                findings.push(Finding { code: "min-wait-loop".into(), severity: Severity::Warning, message: "loop body waits at the workshop minimum rate; the loop runs at maximum frequency".into(), span: program.action_span(rule_id, action_id), rule: rule_id, action: Some(action_id), value: None, evidence: EvidenceClass::StaticIndicator, boundedness: None });
            }
            if config.is_enabled("expensive-loop-check")
                && body.iter().any(action_contains_expensive)
            {
                findings.push(Finding {
                    code: "expensive-loop-check".into(),
                    severity: Severity::Warning,
                    message: "loop body evaluates a potentially expensive geometry predicate"
                        .into(),
                    span: program.action_span(rule_id, action_id),
                    rule: rule_id,
                    action: Some(action_id),
                    value: None,
                    evidence: EvidenceClass::Heuristic,
                    boundedness: None,
                });
            }
            if matches!(action, Action::While { .. })
                && config.is_enabled("while-without-wait")
                && !body.iter().any(|action| is_wait(action, false))
            {
                let boundedness = while_boundedness(action, body);
                findings.push(Finding {
                    code: "while-without-wait".into(),
                    severity: Severity::Warning,
                    message: "loop body contains no wait call and may repeat without yielding"
                        .into(),
                    span: program.action_span(rule_id, action_id),
                    rule: rule_id,
                    action: Some(action_id),
                    value: None,
                    evidence: EvidenceClass::StaticIndicator,
                    boundedness: Some(boundedness),
                });
            }
        }
        if config.is_enabled("duplicate-condition") {
            let mut seen = Vec::new();
            for (condition, value) in rule.conditions.iter().enumerate() {
                if seen
                    .iter()
                    .any(|previous: &&Value| values_equal(previous, &value.value))
                {
                    findings.push(Finding { code: "duplicate-condition".into(), severity: Severity::Warning, message: "condition is evaluated more than once in this rule; a later branch can never be taken".into(), span: program.condition_span(rule_id, condition), rule: rule_id, action: None, value: Some(condition), evidence: EvidenceClass::Exact, boundedness: None });
                }
                seen.push(&value.value);
            }
        }
    }
    let registry = crate::registry::LintRegistry::default();
    for finding in &mut findings {
        if let Some(meta) = registry.rules().find(|meta| meta.id == finding.code) {
            finding.severity = config.effective_severity(meta);
        }
    }
    findings
}

fn loop_body(rule: &Rule, action: usize) -> Option<(usize, usize)> {
    if !matches!(
        rule.actions.get(action),
        Some(
            Action::While { .. }
                | Action::ForGlobalVariable { .. }
                | Action::ForPlayerVariable { .. }
        )
    ) {
        return None;
    }
    let mut depth = 0;
    for index in action + 1..rule.actions.len() {
        match rule.actions[index] {
            Action::If { .. }
            | Action::While { .. }
            | Action::ForGlobalVariable { .. }
            | Action::ForPlayerVariable { .. } => depth += 1,
            Action::End if depth == 0 => return Some((action + 1, index)),
            Action::End => depth -= 1,
            _ => {}
        }
    }
    None
}

fn is_wait(action: &Action, minimum: bool) -> bool {
    matches!(action, Action::Call { name, args } if name == "wait" && (!minimum || matches!(args.first(), Some(Value::Number(value)) if *value <= 0.016)))
}
fn action_contains_expensive(action: &Action) -> bool {
    match action {
        Action::Call { name, args } => {
            ["distance", "raycast", "isInLoS"].contains(&name.as_str())
                || args.iter().any(value_contains_expensive)
        }
        Action::SetGlobalVariable { value, .. } | Action::ModifyGlobalVariable { value, .. } => {
            value_contains_expensive(value)
        }
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => {
            value_contains_expensive(player) || value_contains_expensive(value)
        }
        Action::AssignMember { target, value, .. } => {
            value_contains_expensive(target) || value_contains_expensive(value)
        }
        Action::If { condition } | Action::ElseIf { condition } | Action::While { condition } => {
            value_contains_expensive(condition)
        }
        Action::ForGlobalVariable {
            start, stop, step, ..
        }
        | Action::ForPlayerVariable {
            start, stop, step, ..
        } => [start, stop, step]
            .iter()
            .any(|value| value_contains_expensive(value)),
        Action::Disabled { action } => action_contains_expensive(action),
        _ => false,
    }
}
fn value_contains_expensive(value: &Value) -> bool {
    match value {
        Value::Call { name, args } => {
            ["distance", "raycast", "isInLoS"].contains(&name.as_str())
                || args.iter().any(value_contains_expensive)
        }
        Value::Array(values) => values.iter().any(value_contains_expensive),
        Value::Vector { x, y, z } => [x, y, z]
            .iter()
            .any(|value| value_contains_expensive(value)),
        Value::PlayerVariable { player, .. } => value_contains_expensive(player),
        _ => false,
    }
}
fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b))
        | (Value::LocalizedString(a), Value::LocalizedString(b))
        | (Value::GlobalVariable(a), Value::GlobalVariable(b))
        | (Value::Subroutine(a), Value::Subroutine(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) | (Value::EventPlayer, Value::EventPlayer) => true,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| values_equal(a, b))
        }
        (Value::Call { name: an, args: aa }, Value::Call { name: bn, args: ba }) => {
            an == bn && aa.len() == ba.len() && aa.iter().zip(ba).all(|(a, b)| values_equal(a, b))
        }
        (
            Value::Vector {
                x: ax,
                y: ay,
                z: az,
            },
            Value::Vector {
                x: bx,
                y: by,
                z: bz,
            },
        ) => values_equal(ax, bx) && values_equal(ay, by) && values_equal(az, bz),
        (
            Value::Enum {
                value_type: at,
                value: av,
            },
            Value::Enum {
                value_type: bt,
                value: bv,
            },
        ) => at == bt && av == bv,
        (
            Value::PlayerVariable {
                player: ap,
                variable: av,
            },
            Value::PlayerVariable {
                player: bp,
                variable: bv,
            },
        ) => av == bv && values_equal(ap, bp),
        _ => false,
    }
}
fn while_boundedness(action: &Action, body: &[Action]) -> Boundedness {
    if matches!(
        action,
        Action::While {
            condition: Value::Bool(true)
        }
    ) {
        Boundedness::ObviouslyUnbounded
    } else if body.iter().any(|action| {
        matches!(
            action,
            Action::ModifyGlobalVariable {
                op: ModifyOp::Add | ModifyOp::Subtract,
                value: Value::Number(_),
                ..
            }
        )
    }) {
        Boundedness::StaticallyBounded
    } else {
        Boundedness::Unknown
    }
}

fn cfg_response(program: &Program, rule: RuleId) -> Response {
    let Some(data) = program.rules.get(rule) else {
        return Response::Error {
            error: ErrorInfo {
                code: "invalid-id".into(),
                message: format!("unknown rule {rule}"),
            },
        };
    };
    let loop_actions: Vec<(usize, &str)> = data
        .actions
        .iter()
        .enumerate()
        .filter_map(|(action, action_data)| {
            let kind = match action_data {
                Action::While { .. } => "while",
                Action::ForGlobalVariable { .. } | Action::ForPlayerVariable { .. } => "for",
                _ => return None,
            };
            Some((action, kind))
        })
        .collect();
    let exit = loop_actions.len() + 1;
    let mut blocks = vec![json!({
        "id": 0,
        "kind": "entry",
        "waits": data.actions.iter().any(|action| is_wait(action, false)),
        "calls": [],
        "actions": if loop_actions.is_empty() { (0..data.actions.len()).collect::<Vec<_>>() } else { Vec::new() },
        "successors": [{"to": 1, "kind": "fallthrough"}]
    })];
    for (offset, (action, kind)) in loop_actions.iter().enumerate() {
        blocks.push(json!({
            "id": offset + 1,
            "kind": kind,
            "waits": is_wait(&data.actions[*action], false),
            "calls": [],
            "actions": [action],
            "successors": [
                {"to": offset + 1, "kind": "back-edge"},
                {"to": if offset + 1 == exit { exit } else { offset + 2 }, "kind": "fallthrough"}
            ]
        }));
    }
    blocks.push(json!({"id": exit, "kind": "exit", "waits": false, "calls": [], "actions": [], "successors": []}));
    Response::Ok {
        result: json!({"entry": 0, "exit": exit, "blocks": blocks}),
    }
}

fn persistent_objects(program: &Program) -> Vec<JsonValue> {
    let mut output = Vec::new();
    for (rule, data) in program.rules.iter().enumerate() {
        for (action, action_data) in data.actions.iter().enumerate() {
            let Action::Call { name, args } = action_data else {
                continue;
            };
            let Some(kind) = persistent_object_kind(name) else {
                continue;
            };
            let cleanup = match kind {
                "hud-text" => "destroyHudText",
                "in-world-text" => "destroyInWorldText",
                "effect" => "destroyEffect",
                _ => continue,
            };
            let reevaluation_index = if kind == "hud-text" { 9 } else { 5 };
            let reevaluation = args.get(reevaluation_index).and_then(|value| match value {
                Value::Enum { value_type, value } => {
                    Some(json!({"domain": value_type, "mode": value}))
                }
                _ => None,
            });
            let span = span_json(program.action_span(rule, action));
            output.push(json!({
                "kind": kind,
                "rule": rule,
                "action": action,
                "executionScope": execution_scope(&data.event),
                "visibility": object_visibility(args),
                "reevaluation": reevaluation,
                "identityRetained": true,
                "sameKindCleanupInRule": data.actions.iter().any(|action| matches!(action, Action::Call { name, .. } if name == cleanup)),
                "span": span,
            }));
        }
    }
    output
}

fn persistent_object_kind(name: &str) -> Option<&'static str> {
    match name {
        "createHudText" => Some("hud-text"),
        "createInWorldText" => Some("in-world-text"),
        "createEffect" => Some("effect"),
        _ => None,
    }
}

fn execution_scope(event: &Event) -> &'static str {
    match event {
        Event::Global => "global",
        Event::EachPlayer => "per-player",
        Event::Subroutine(_) => "subroutine",
        _ => "unknown",
    }
}

fn object_visibility(args: &[Value]) -> &'static str {
    match args.first() {
        Some(Value::EventPlayer) => "event-player",
        Some(Value::Array(_)) => "explicit-set",
        Some(Value::Call { name, .. }) if name == "allPlayers" => "all-players",
        Some(Value::Call { .. } | Value::PlayerVariable { .. } | Value::GlobalVariable(_)) => {
            "dynamic"
        }
        _ => "unknown",
    }
}
fn lint_rules(registry: &crate::registry::LintRegistry, config: &LintConfig) -> JsonValue {
    let descriptors = registry.descriptors(config);
    let rules = descriptors
        .iter()
        .map(|rule| {
            json!({
                "id": rule.id,
                "defaultSeverity": rule.default_severity,
                "effectiveSeverity": rule.effective_severity,
                "enabled": rule.enabled,
                "summary": rule.summary,
                "rationale": rule.rationale,
                "documentation": rule.documentation,
                "knownLimits": rule.known_limits,
                "evidence": rule.evidence,
                "tags": rule.tags,
                "kind": rule.kind,
            })
        })
        .collect::<Vec<_>>();
    let config_rules = descriptors
        .iter()
        .map(|rule| {
            (
                rule.id.clone(),
                json!({
                    "enabled": rule.enabled,
                    "severity": severity_name(rule.effective_severity),
                    "options": config.options(&rule.id),
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({
        "rules": rules,
        "config": {"rules": config_rules},
        "skipped": [],
    })
}
fn file_count(_program: &Program) -> usize {
    1
}
fn span_json(span: Option<Span>) -> JsonValue {
    span.map_or(JsonValue::Null, |span| json!({"file": span.file.index(), "start": {"line": span.start.line, "col": span.start.col}, "end": {"line": span.end.line, "col": span.end.col}}))
}
fn symbol_json(symbol: &Symbol) -> JsonValue {
    json!({"id": symbol.id.index(), "kind": symbol_kind_name(symbol.kind), "name": symbol.name, "span": span_json(symbol.span)})
}
fn finding_json(finding: &Finding) -> JsonValue {
    json!({"code": finding.code, "severity": severity_name(finding.severity), "message": finding.message, "span": span_json(finding.span), "rule": finding.rule, "action": finding.action, "value": finding.value, "evidence": finding.evidence.as_str(), "boundedness": finding.boundedness.map(Boundedness::as_str)})
}
fn symbol_kind_name(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::GlobalVariable => "globalVariable",
        SymbolKind::PlayerVariable => "playerVariable",
        SymbolKind::Subroutine => "subroutine",
        SymbolKind::Rule => "rule",
    }
}
fn reference_kind_name(kind: ReferenceKind) -> &'static str {
    match kind {
        ReferenceKind::Declaration => "declaration",
        ReferenceKind::Definition => "definition",
        ReferenceKind::Read => "read",
        ReferenceKind::Write => "write",
        ReferenceKind::Call => "call",
    }
}
fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}
fn event_name(event: &Event) -> String {
    match event {
        Event::Global => "global".into(),
        Event::EachPlayer | Event::EachPlayerWithFilters { .. } => "eachPlayer".into(),
        Event::Player { kind, .. } => format!("{kind:?}"),
        Event::Subroutine(name) => format!("subroutine:{name}"),
    }
}
