use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::{Value as JsonValue, json};
use workshop_rs::source::Span;
use workshop_rs::{Action, Event, ModifyOp, Program, Rule, Value};

use crate::analysis::{Boundedness, EvidenceClass, Severity};
use crate::registry::LintConfig;
use crate::service::{ErrorInfo, Origin, Request, Response};

pub type RuleId = usize;
pub type ActionId = usize;
pub type ValueId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub usize);

impl SymbolId {
    pub const fn from_index(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

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
                    program.action_argument_span(rule, action_id, 0),
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
                    program.action_argument_span(rule, action_id, 1),
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
                    program.action_argument_span(rule, action_id, 0),
                    program,
                );
                self.walk_value(
                    stop,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 1),
                    program,
                );
                self.walk_value(
                    step,
                    rule,
                    Some(action_id),
                    None,
                    program.action_argument_span(rule, action_id, 2),
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
    let Some(source_doc) = program.source(span.file) else {
        return Some(span);
    };
    let source = source_doc.text();
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
        if let Some(operator) = line.find('=') {
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
    Some(span)
}

fn value_occurrence(program: &Program, span: Option<Span>, name: &str) -> Option<Span> {
    let span = span?;
    let Some(source_doc) = program.source(span.file) else {
        return Some(span);
    };
    let source = source_doc.text();
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
    Some(span)
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
    pub fn new(program: &'a Program) -> Self {
        Self::with_origin(
            program,
            Origin {
                kind: "unknown".to_string(),
                locale: None,
            },
        )
    }
    pub fn from_workshop(program: &'a Program, locale: &str) -> Self {
        Self::with_origin(
            program,
            Origin {
                kind: "workshop".to_string(),
                locale: Some(workshop_rs::catalog::Locale::new(locale).to_string()),
            },
        )
    }
    pub fn with_origin(program: &'a Program, origin: Origin) -> Self {
        Self::with_origin_and_config(program, origin, LintConfig::default())
    }
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
    pub fn handle_json(&self, request_json: &str) -> String {
        let request: Request = match serde_json::from_str(request_json) {
            Ok(req) => req,
            Err(err) => {
                return serde_json::to_string(&Response::Error {
                    error: ErrorInfo {
                        code: "invalid-json".to_string(),
                        message: format!("could not parse request JSON: {err}"),
                    },
                })
                .expect("error response serializes");
            }
        };
        let response = self.handle(&request);
        serde_json::to_string(&response).expect("response serializes")
    }
    pub fn handle(&self, request: &Request) -> Response {
        match request {
            Request::Version => Response::Ok { result: json!({"name": "wright-tool", "version": env!("CARGO_PKG_VERSION"), "capabilities": ["program", "rules", "symbols", "references", "usage", "cfg", "findings", "persistentObjects", "lintRules"]}) },
            Request::Program => Response::Ok { result: json!({"origin": self.origin, "files": file_count(self.program), "globalVariables": self.program.global_variables.len(), "playerVariables": self.program.player_variables.len(), "subroutines": self.program.subroutines.len(), "rules": self.program.rules.len(), "findings": self.findings.len()}) },
            Request::ListRules => Response::Ok { result: json!(self.program.rules.iter().enumerate().map(|(id, rule)| json!({"id": id, "name": rule.name, "span": span_json(self.program.rule_span(id))})).collect::<Vec<_>>()) },
            Request::GetRule { rule } => self.rule(*rule as usize),
            Request::ListSymbols { kind } => Response::Ok { result: json!(self.index.symbols().filter(|symbol| kind.as_deref().is_none_or(|kind| symbol_kind_name(symbol.kind) == kind)).map(symbol_json).collect::<Vec<_>>()) },
            Request::GetSymbol { symbol } => self.index.symbol(SymbolId::from_index(*symbol as usize)).map_or_else(|| self.error("invalid-id", format!("unknown symbol {symbol}")), |symbol| Response::Ok { result: symbol_json(symbol) }),
            Request::FindReferences { symbol } => { let id = SymbolId::from_index(*symbol as usize); if self.index.symbol(id).is_none() { self.error("invalid-id", format!("unknown symbol {symbol}")) } else { Response::Ok { result: json!(self.index.references(id).into_iter().map(|reference| json!({"kind": reference_kind_name(reference.kind), "span": span_json(reference.span), "rule": reference.rule, "action": reference.action, "value": reference.value})).collect::<Vec<_>>()) } } }
            Request::GetUsage { symbol } => { let id = SymbolId::from_index(*symbol as usize); self.index.symbol(id).map_or_else(|| self.error("invalid-id", format!("unknown symbol {symbol}")), |data| { let usage = self.index.usage(id); Response::Ok { result: json!({"symbol": data.name, "reads": usage.reads, "writes": usage.writes, "calls": usage.calls, "rules": usage.rules}) } }) }
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
        if config.is_enabled("ongoing-condition-hot-path") {
            findings.extend(ongoing_condition_findings(program, rule_id, rule));
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
            if let Action::While { condition } = action {
                if config.is_enabled("while-without-wait")
                    && !body.iter().any(|action| is_wait(action, false))
                {
                    let boundedness = while_boundedness(condition, body, &program.subroutines);
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
            if config.is_enabled("repeated-value")
                && matches!(
                    action,
                    Action::While { .. } | Action::ForGlobalVariable { .. }
                )
            {
                findings.extend(repeated_value_findings(
                    program, rule_id, rule, action_id, start, body,
                ));
            }
        }
        if config.is_enabled("duplicate-condition") {
            findings.extend(duplicate_condition_findings(program, rule_id, rule));
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
fn ongoing_condition_findings(program: &Program, rule_id: RuleId, rule: &Rule) -> Vec<Finding> {
    if !matches!(
        &rule.event,
        Event::Global | Event::EachPlayer | Event::EachPlayerWithFilters { .. }
    ) {
        return Vec::new();
    }

    let active_conditions: Vec<_> = rule
        .conditions
        .iter()
        .enumerate()
        .filter(|(_, condition)| !condition.disabled)
        .collect();
    let condition_count = active_conditions.len();
    let mut findings = Vec::new();
    for (index, &(source_index, condition)) in active_conditions.iter().enumerate() {
        let mut expensive = Vec::new();
        collect_expensive_values(&condition.value, &mut expensive);
        for value in expensive {
            let preceding = index;
            let later = condition_count - index - 1;
            let evaluation = match preceding {
                0 => "is evaluated every server tick".to_string(),
                1 => "is evaluated only after 1 preceding condition passes".to_string(),
                count => format!("is evaluated only after {count} preceding conditions pass"),
            };
            let later_gates = if later == 0 {
                String::new()
            } else {
                format!(
                    ", before {later} later short-circuit gate{}",
                    if later == 1 { "" } else { "s" }
                )
            };
            let name = match value {
                Value::Call { name, .. } => name,
                _ => unreachable!("only expensive call values are collected"),
            };
            let span = program.condition_span(rule_id, source_index);
            findings.push(Finding {
                code: "ongoing-condition-hot-path".into(),
                severity: Severity::Info,
                message: format!(
                    "geometry predicate in an ongoing-rule condition {} of {condition_count} {evaluation}{later_gates}; its cost is heuristic, not measured runtime load",
                    index + 1,
                ),
                span: value_occurrence(program, span, name).or(span),
                rule: rule_id,
                action: None,
                value: None,
                evidence: EvidenceClass::Heuristic,
                boundedness: None,
            });
        }
    }
    findings
}

fn collect_expensive_values<'a>(value: &'a Value, out: &mut Vec<&'a Value>) {
    match value {
        Value::Call { name, args } => {
            if ["distance", "raycast", "isInLoS"].contains(&name.as_str()) {
                out.push(value);
            }
            for argument in args {
                collect_expensive_values(argument, out);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_expensive_values(value, out);
            }
        }
        Value::Vector { x, y, z } => {
            collect_expensive_values(x, out);
            collect_expensive_values(y, out);
            collect_expensive_values(z, out);
        }
        Value::PlayerVariable { player, .. } => collect_expensive_values(player, out),
        _ => {}
    }
}

fn duplicate_condition_findings(program: &Program, rule_id: RuleId, rule: &Rule) -> Vec<Finding> {
    let mut seen: Vec<&Value> = Vec::new();
    let mut findings = Vec::new();
    for (action_id, action) in rule.actions.iter().enumerate() {
        let condition = match action {
            Action::If { condition }
            | Action::ElseIf { condition }
            | Action::While { condition } => condition,
            _ => continue,
        };
        if seen
            .iter()
            .any(|previous| values_equal(previous, condition))
        {
            let span = program.action_argument_span(rule_id, action_id, 0);
            findings.push(Finding {
                code: "duplicate-condition".into(),
                severity: Severity::Warning,
                message: "condition is evaluated more than once in this rule; a later branch can never be taken".into(),
                span: span.or_else(|| program.action_span(rule_id, action_id)),
                rule: rule_id,
                action: Some(action_id),
                value: None,
                evidence: EvidenceClass::Exact,
                boundedness: None,
            });
        } else {
            seen.push(condition);
        }
    }
    findings
}

fn repeated_value_findings(
    program: &Program,
    rule_id: RuleId,
    rule: &Rule,
    loop_action: ActionId,
    body_start: usize,
    body: &[Action],
) -> Vec<Finding> {
    let mut values = Vec::new();
    let mut parents = Vec::new();
    let mut spans = Vec::new();
    if let Action::While { condition } = &rule.actions[loop_action] {
        collect_value_tree(
            condition,
            None,
            program.action_argument_span(rule_id, loop_action, 0),
            &mut values,
            &mut parents,
            &mut spans,
        );
    }
    let mut action = 0;
    while action < body.len() {
        if matches!(
            body[action],
            Action::While { .. }
                | Action::ForGlobalVariable { .. }
                | Action::ForPlayerVariable { .. }
        ) {
            action = matching_end(body, action).map_or(action + 1, |end| end + 1);
            continue;
        }
        let action_id = body_start + action;
        visit_action_roots(&body[action], &mut |argument, value| {
            collect_value_tree(
                value,
                None,
                program.action_argument_span(rule_id, action_id, argument),
                &mut values,
                &mut parents,
                &mut spans,
            );
        });
        action += 1;
    }

    duplicated_value_families(&values, &parents)
        .into_iter()
        .map(|family| {
            let first = family[0];
            Finding {
                code: "repeated-value".into(),
                severity: Severity::Warning,
                message: format!(
                    "this value expression is evaluated {} times within the same loop scope",
                    family.len()
                ),
                span: spans[first].or_else(|| program.action_span(rule_id, loop_action)),
                rule: rule_id,
                action: Some(loop_action),
                value: None,
                evidence: EvidenceClass::Exact,
                boundedness: None,
            }
        })
        .collect()
}

fn visit_action_roots<'a>(action: &'a Action, visit: &mut impl FnMut(usize, &'a Value)) {
    match action {
        Action::SetGlobalVariable { value, .. } | Action::ModifyGlobalVariable { value, .. } => {
            visit(0, value)
        }
        Action::SetPlayerVariable { player, value, .. }
        | Action::ModifyPlayerVariable { player, value, .. } => {
            visit(0, player);
            visit(1, value);
        }
        Action::AssignMember { target, value, .. } => {
            visit(0, target);
            visit(1, value);
        }
        Action::If { condition } | Action::ElseIf { condition } | Action::While { condition } => {
            visit(0, condition);
        }
        Action::ForGlobalVariable {
            start, stop, step, ..
        } => {
            visit(0, start);
            visit(1, stop);
            visit(2, step);
        }
        Action::ForPlayerVariable {
            player,
            start,
            stop,
            step,
            ..
        } => {
            visit(0, player);
            visit(1, start);
            visit(2, stop);
            visit(3, step);
        }
        Action::Call { args, .. } => {
            for (index, value) in args.iter().enumerate() {
                visit(index, value);
            }
        }
        Action::CallSubroutine { .. } | Action::Else | Action::End | Action::Disabled { .. } => {}
    }
}

fn collect_value_tree<'a>(
    value: &'a Value,
    parent: Option<usize>,
    span: Option<Span>,
    values: &mut Vec<&'a Value>,
    parents: &mut Vec<Option<usize>>,
    spans: &mut Vec<Option<Span>>,
) {
    let index = values.len();
    values.push(value);
    parents.push(parent);
    spans.push(span);
    match value {
        Value::Array(children) | Value::Call { args: children, .. } => {
            for child in children {
                collect_value_tree(child, Some(index), span, values, parents, spans);
            }
        }
        Value::Vector { x, y, z } => {
            for child in [x, y, z] {
                collect_value_tree(child, Some(index), span, values, parents, spans);
            }
        }
        Value::PlayerVariable { player, .. } => {
            collect_value_tree(player, Some(index), span, values, parents, spans);
        }
        _ => {}
    }
}

fn duplicated_value_families(values: &[&Value], parents: &[Option<usize>]) -> Vec<Vec<usize>> {
    let mut families: Vec<(usize, Vec<usize>)> = Vec::new();
    for (position, value) in values.iter().enumerate() {
        if let Some((_, family)) = families
            .iter_mut()
            .find(|(_, family)| values_equal(values[family[0]], value))
        {
            family.push(position);
        } else {
            families.push((position, vec![position]));
        }
    }

    let candidates: Vec<usize> = families
        .iter()
        .enumerate()
        .filter(|(_, (_, family))| family.len() >= 2 && value_call_count(values[family[0]]) >= 2)
        .map(|(index, _)| index)
        .collect();
    let mut member_family = vec![None; values.len()];
    for &family in &candidates {
        for &member in &families[family].1 {
            member_family[member] = Some(family);
        }
    }
    let mut reported: Vec<usize> = candidates
        .into_iter()
        .filter(|&family| {
            !families[family].1.iter().any(|&member| {
                let mut ancestor = parents[member];
                while let Some(index) = ancestor {
                    if member_family[index].is_some_and(|other| other != family) {
                        return true;
                    }
                    ancestor = parents[index];
                }
                false
            })
        })
        .collect();
    reported.sort_by_key(|&family| families[family].0);
    reported
        .into_iter()
        .map(|family| std::mem::take(&mut families[family].1))
        .collect()
}

fn value_call_count(value: &Value) -> usize {
    match value {
        Value::Call { args, .. } => 1 + args.iter().map(value_call_count).sum::<usize>(),
        Value::Array(values) => values.iter().map(value_call_count).sum(),
        Value::Vector { x, y, z } => {
            value_call_count(x) + value_call_count(y) + value_call_count(z)
        }
        Value::PlayerVariable { player, .. } => value_call_count(player),
        _ => 0,
    }
}

fn while_boundedness(
    condition: &Value,
    body: &[Action],
    subroutines: &[workshop_rs::Subroutine],
) -> Boundedness {
    if matches!(condition, Value::Bool(true)) {
        return Boundedness::ObviouslyUnbounded;
    }
    let Some((variable, toward)) = counter_comparison(condition) else {
        return Boundedness::Unknown;
    };
    let mut progresses = false;
    for (start, end) in direct_action_ranges(body) {
        if action_has_unprovable_loop(body, start, end, subroutines) {
            return Boundedness::Unknown;
        }
        match modify_direction(&body[start], &variable) {
            Some(direction) if direction == toward => progresses = true,
            Some(_) => return Boundedness::Unknown,
            None if region_writes(&body[start..end], &variable, subroutines) => {
                return Boundedness::Unknown;
            }
            None => {}
        }
    }
    if progresses {
        Boundedness::StaticallyBounded
    } else {
        Boundedness::Unknown
    }
}

#[derive(Debug, Clone)]
enum CounterVariable {
    Global(String),
    Player { player: Value, variable: String },
}

fn counter_comparison(condition: &Value) -> Option<(CounterVariable, i32)> {
    let Value::Call { name, args } = condition else {
        return None;
    };
    if args.len() != 2 {
        return None;
    }
    let left_variable = variable_of(&args[0]);
    let right_variable = variable_of(&args[1]);
    let left_literal = matches!(&args[0], Value::Number(_));
    let right_literal = matches!(&args[1], Value::Number(_));
    let (variable, variable_is_left) = match (left_variable, right_literal) {
        (Some(variable), true) => (variable, true),
        (None, false) if left_literal => (right_variable?, false),
        _ => return None,
    };
    let direction = match (name.as_str(), variable_is_left) {
        ("<", true) | ("<=", true) | (">", false) | (">=", false) => 1,
        (">", true) | (">=", true) | ("<", false) | ("<=", false) => -1,
        _ => return None,
    };
    Some((variable, direction))
}

fn variable_of(value: &Value) -> Option<CounterVariable> {
    match value {
        Value::GlobalVariable(name) => Some(CounterVariable::Global(name.clone())),
        Value::PlayerVariable { player, variable } => Some(CounterVariable::Player {
            player: player.as_ref().clone(),
            variable: variable.clone(),
        }),
        _ => None,
    }
}

fn modify_direction(action: &Action, variable: &CounterVariable) -> Option<i32> {
    let (op, value) = match action {
        Action::ModifyGlobalVariable {
            variable: target,
            op,
            value,
        } if matches!(variable, CounterVariable::Global(name) if name == target) => (op, value),
        Action::ModifyPlayerVariable {
            player,
            variable: target,
            op,
            value,
        } => {
            let CounterVariable::Player {
                player: expected_player,
                variable: expected_variable,
            } = variable
            else {
                return None;
            };
            if target != expected_variable || !values_equal(player, expected_player) {
                return None;
            }
            (op, value)
        }
        _ => return None,
    };
    let Value::Number(step) = value else {
        return None;
    };
    if *step == 0.0 {
        return None;
    }
    let sign = if *step > 0.0 { 1 } else { -1 };
    match op {
        ModifyOp::Add => Some(sign),
        ModifyOp::Subtract => Some(-sign),
        _ => None,
    }
}

fn region_writes(
    actions: &[Action],
    variable: &CounterVariable,
    subroutines: &[workshop_rs::Subroutine],
) -> bool {
    actions
        .iter()
        .any(|action| action_writes(action, variable, subroutines))
}

fn action_writes(
    action: &Action,
    variable: &CounterVariable,
    subroutines: &[workshop_rs::Subroutine],
) -> bool {
    match action {
        Action::SetGlobalVariable {
            variable: target, ..
        }
        | Action::ModifyGlobalVariable {
            variable: target, ..
        } => {
            matches!(variable, CounterVariable::Global(name) if name == target)
        }
        Action::SetPlayerVariable {
            player,
            variable: target,
            ..
        }
        | Action::ModifyPlayerVariable {
            player,
            variable: target,
            ..
        } => {
            matches!(variable, CounterVariable::Player { player: expected_player, variable: expected_variable }
            if target == expected_variable && values_equal(player, expected_player))
        }
        Action::CallSubroutine { .. } | Action::AssignMember { .. } => true,
        Action::Call { name, .. } => subroutines
            .iter()
            .any(|subroutine| subroutine.name == *name),
        Action::Disabled { action } => action_writes(action, variable, subroutines),
        _ => false,
    }
}

fn action_has_unprovable_loop(
    actions: &[Action],
    start: usize,
    end: usize,
    subroutines: &[workshop_rs::Subroutine],
) -> bool {
    let inner = if end > start + 1 {
        &actions[start + 1..end - 1]
    } else {
        &[]
    };
    match &actions[start] {
        Action::While { condition } => {
            while_boundedness(condition, inner, subroutines) != Boundedness::StaticallyBounded
                || region_has_unprovable_loop(inner, subroutines)
        }
        Action::ForGlobalVariable { variable, step, .. } => {
            let finite_step = step_direction(step).is_some();
            !finite_step
                || region_writes(
                    inner,
                    &CounterVariable::Global(variable.clone()),
                    subroutines,
                )
                || region_has_unprovable_loop(inner, subroutines)
        }
        Action::ForPlayerVariable { .. } => true,
        Action::If { .. } => region_has_unprovable_loop(inner, subroutines),
        _ => false,
    }
}

fn region_has_unprovable_loop(actions: &[Action], subroutines: &[workshop_rs::Subroutine]) -> bool {
    direct_action_ranges(actions)
        .into_iter()
        .any(|(start, end)| action_has_unprovable_loop(actions, start, end, subroutines))
}

fn direct_action_ranges(actions: &[Action]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < actions.len() {
        let end = matching_end(actions, start).map_or(start + 1, |end| end + 1);
        ranges.push((start, end));
        start = end;
    }
    ranges
}

fn matching_end(actions: &[Action], start: usize) -> Option<usize> {
    if !matches!(
        actions.get(start),
        Some(
            Action::If { .. }
                | Action::While { .. }
                | Action::ForGlobalVariable { .. }
                | Action::ForPlayerVariable { .. }
        )
    ) {
        return None;
    }
    let mut depth = 0;
    for (index, action) in actions.iter().enumerate().skip(start + 1) {
        match action {
            Action::If { .. }
            | Action::While { .. }
            | Action::ForGlobalVariable { .. }
            | Action::ForPlayerVariable { .. } => depth += 1,
            Action::End if depth == 0 => return Some(index),
            Action::End => depth -= 1,
            _ => {}
        }
    }
    None
}

fn step_direction(step: &Value) -> Option<i32> {
    let Value::Number(step) = step else {
        return None;
    };
    if *step == 0.0 {
        None
    } else {
        Some(if *step > 0.0 { 1 } else { -1 })
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
    let mut builder = CanonicalCfgBuilder {
        program,
        actions: &data.actions,
        blocks: Vec::new(),
    };
    let (entry, terminal) = builder.sequence(0, data.actions.len(), "entry");
    let exit = builder.new_block("exit");
    builder.edge(terminal, exit, "fallthrough");
    Response::Ok {
        result: json!({
            "entry": entry,
            "exit": exit,
            "blocks": builder.blocks.iter().enumerate().map(|(id, block)| json!({
                "id": id,
                "kind": block.kind,
                "waits": block.waits,
                "calls": block.calls,
                "actions": block.actions,
                "successors": block.successors.iter().map(|(to, kind)| json!({"to": to, "kind": kind})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }),
    }
}

struct CanonicalCfgBlock {
    kind: &'static str,
    waits: bool,
    calls: Vec<usize>,
    actions: Vec<usize>,
    successors: Vec<(usize, &'static str)>,
}

struct CanonicalCfgBuilder<'a> {
    program: &'a Program,
    actions: &'a [Action],
    blocks: Vec<CanonicalCfgBlock>,
}

struct IfBranch {
    condition_action: usize,
    body_start: usize,
    body_end: usize,
}

struct IfParts {
    close: usize,
    branches: Vec<IfBranch>,
    else_body: Option<(usize, usize)>,
}

impl CanonicalCfgBuilder<'_> {
    fn new_block(&mut self, kind: &'static str) -> usize {
        let id = self.blocks.len();
        self.blocks.push(CanonicalCfgBlock {
            kind,
            waits: false,
            calls: Vec::new(),
            actions: Vec::new(),
            successors: Vec::new(),
        });
        id
    }

    fn edge(&mut self, from: usize, to: usize, kind: &'static str) {
        self.blocks[from].successors.push((to, kind));
    }

    fn sequence(&mut self, start: usize, end: usize, entry_kind: &'static str) -> (usize, usize) {
        let entry = self.new_block(entry_kind);
        let mut current = entry;
        let mut index = start;
        while index < end {
            if matches!(self.actions[index], Action::If { .. }) {
                if let Some(parts) = if_parts(self.actions, index) {
                    let merge = self.new_block("block");
                    let mut false_target = if let Some((else_start, else_end)) = parts.else_body {
                        let (else_entry, else_exit) = self.sequence(else_start, else_end, "block");
                        self.edge(else_exit, merge, "fallthrough");
                        Some(else_entry)
                    } else {
                        None
                    };
                    for branch_data in parts.branches.into_iter().rev() {
                        let branch = self.new_block("if");
                        self.blocks[branch]
                            .actions
                            .push(branch_data.condition_action);
                        let (body_entry, body_exit) =
                            self.sequence(branch_data.body_start, branch_data.body_end, "block");
                        self.edge(branch, body_entry, "true");
                        self.edge(body_exit, merge, "fallthrough");
                        self.edge(branch, false_target.unwrap_or(merge), "false");
                        false_target = Some(branch);
                    }
                    self.edge(current, false_target.unwrap_or(merge), "fallthrough");
                    current = merge;
                    index = parts.close + 1;
                    continue;
                }
            }
            if matches!(
                self.actions[index],
                Action::While { .. }
                    | Action::ForGlobalVariable { .. }
                    | Action::ForPlayerVariable { .. }
            ) {
                if let Some(close) = matching_end(self.actions, index) {
                    let kind = if matches!(self.actions[index], Action::While { .. }) {
                        "while"
                    } else {
                        "for"
                    };
                    let header = self.new_block(kind);
                    self.blocks[header].actions.push(index);
                    self.edge(current, header, "fallthrough");
                    let (body_entry, body_exit) = self.sequence(index + 1, close, "block");
                    self.edge(
                        header,
                        body_entry,
                        if kind == "while" {
                            "true"
                        } else {
                            "fallthrough"
                        },
                    );
                    self.edge(body_exit, header, "back");
                    let after = self.new_block("block");
                    self.edge(header, after, "loop-exit");
                    current = after;
                    index = close + 1;
                    continue;
                }
            }
            match &self.actions[index] {
                Action::Else | Action::ElseIf { .. } | Action::End => {}
                action => {
                    let block = &mut self.blocks[current];
                    block.actions.push(index);
                    if is_wait(action, false) {
                        block.waits = true;
                    }
                    if let Some(subroutine) = action_subroutine(self.program, action) {
                        block.calls.push(subroutine);
                    }
                }
            }
            index += 1;
        }
        (entry, current)
    }
}

fn if_parts(actions: &[Action], start: usize) -> Option<IfParts> {
    let close = matching_end(actions, start)?;
    let mut branches = Vec::new();
    let mut condition_action = start;
    let mut body_start = start + 1;
    let mut index = body_start;
    let mut else_body = None;
    while index < close {
        match actions[index] {
            Action::ElseIf { .. } => {
                branches.push(IfBranch {
                    condition_action,
                    body_start,
                    body_end: index,
                });
                condition_action = index;
                body_start = index + 1;
            }
            Action::Else => {
                branches.push(IfBranch {
                    condition_action,
                    body_start,
                    body_end: index,
                });
                else_body = Some((index + 1, close));
                break;
            }
            _ => {
                if let Some(nested_close) = matching_end(actions, index) {
                    index = nested_close + 1;
                    continue;
                }
            }
        }
        index += 1;
    }
    if else_body.is_none() {
        branches.push(IfBranch {
            condition_action,
            body_start,
            body_end: close,
        });
    }
    Some(IfParts {
        close,
        branches,
        else_body,
    })
}

fn action_subroutine(program: &Program, action: &Action) -> Option<usize> {
    let name = match action {
        Action::CallSubroutine { subroutine } => subroutine,
        Action::Call { name, .. } => name,
        _ => return None,
    };
    program
        .subroutines
        .iter()
        .position(|subroutine| subroutine.name == *name)
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
            let identity = match kind {
                "hud-text" | "in-world-text" => "lastTextId",
                "effect" => "lastCreatedEntity",
                _ => unreachable!(),
            };
            let identity_retained = data
                .actions
                .get(action + 1)
                .is_some_and(|next| action_retains_identity(next, identity));
            let span = span_json(program.action_span(rule, action));
            output.push(json!({
                "kind": kind,
                "rule": rule,
                "action": action,
                "executionScope": execution_scope(&data.event),
                "visibility": object_visibility(args),
                "reevaluation": reevaluation,
                "identityRetained": identity_retained,
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
        Event::EachPlayer | Event::EachPlayerWithFilters { .. } | Event::Player { .. } => {
            "per-player"
        }
        Event::Subroutine(_) => "subroutine",
    }
}

fn action_retains_identity(action: &Action, identity: &str) -> bool {
    let value = match action {
        Action::SetGlobalVariable { value, .. }
        | Action::SetPlayerVariable { value, .. }
        | Action::AssignMember { value, .. } => value,
        _ => return false,
    };
    matches!(value, Value::Call { name, args } if name == identity && args.is_empty())
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
fn file_count(_program: &Program) -> Option<usize> {
    // The public Program API exposes source lookup by FileId but not a file
    // iterator or count, so the exact count is unavailable at this boundary.
    None
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
