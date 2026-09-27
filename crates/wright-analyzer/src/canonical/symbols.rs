use std::collections::{HashMap, HashSet};

use workshop_rs::source::Span;
use workshop_rs::{Action, Event, Program, Value};

use super::traversal::{visit_action_roots, visit_value_tree};

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

fn append_named_symbols<'a>(
    symbols: &mut Vec<Symbol>,
    program: &Program,
    kind: SymbolKind,
    names: impl Iterator<Item = &'a str>,
) {
    let prefix = kind.declaration_prefix();
    for name in names {
        let id = SymbolId::from_index(symbols.len());
        let occurrence = declaration_span(program, prefix, name);
        symbols.push(Symbol {
            id,
            kind,
            name: name.to_string(),
            span: occurrence,
            occurrence,
            rule: None,
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    GlobalVariable,
    PlayerVariable,
    Subroutine,
    Rule,
}

impl SymbolKind {
    pub const ALL: [Self; 4] = [
        Self::GlobalVariable,
        Self::PlayerVariable,
        Self::Subroutine,
        Self::Rule,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GlobalVariable => "globalVariable",
            Self::PlayerVariable => "playerVariable",
            Self::Subroutine => "subroutine",
            Self::Rule => "rule",
        }
    }

    pub const fn declaration_prefix(self) -> &'static str {
        match self {
            Self::GlobalVariable => "globalvar ",
            Self::PlayerVariable => "playervar ",
            Self::Subroutine => "subroutine ",
            Self::Rule => "rule ",
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
    next_value_id: ValueId,
}

pub(super) fn value_identity_map(program: &Program) -> HashMap<usize, ValueId> {
    let mut identities = HashMap::new();
    for rule in &program.rules {
        for condition in &rule.conditions {
            visit_value_tree(&condition.value, None, &mut |value, _| {
                let id = identities.len();
                identities.insert((value as *const Value) as usize, id);
                0
            });
        }
        for action in &rule.actions {
            visit_action_roots(action, &mut |_, root| {
                visit_value_tree(root, None, &mut |value, _| {
                    let id = identities.len();
                    identities.insert((value as *const Value) as usize, id);
                    0
                });
            });
        }
    }
    identities
}

impl SemanticIndex {
    pub fn build(program: &Program) -> Self {
        let mut symbols = Vec::new();
        append_named_symbols(
            &mut symbols,
            program,
            SymbolKind::GlobalVariable,
            program
                .global_variables
                .iter()
                .map(|variable| variable.name.as_str()),
        );
        append_named_symbols(
            &mut symbols,
            program,
            SymbolKind::PlayerVariable,
            program
                .player_variables
                .iter()
                .map(|variable| variable.name.as_str()),
        );
        append_named_symbols(
            &mut symbols,
            program,
            SymbolKind::Subroutine,
            program
                .subroutines
                .iter()
                .map(|subroutine| subroutine.name.as_str()),
        );
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
            next_value_id: 0,
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
    fn find_symbol(&self, kind: SymbolKind, name: &str) -> Option<SymbolId> {
        self.symbols
            .iter()
            .find(|symbol| symbol.kind == kind && symbol.name == name)
            .map(|s| s.id)
    }
    fn walk_event(&mut self, event: &Event, rule: RuleId, program: &Program) {
        if let Event::Subroutine(name) = event {
            if let Some(symbol) = self.find_symbol(SymbolKind::Subroutine, name) {
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
        let variable = match action {
            Action::SetGlobalVariable { variable, .. } => {
                Some((SymbolKind::GlobalVariable, variable, false))
            }
            Action::ModifyGlobalVariable { variable, .. } => {
                Some((SymbolKind::GlobalVariable, variable, true))
            }
            Action::SetPlayerVariable { variable, .. } => {
                Some((SymbolKind::PlayerVariable, variable, false))
            }
            Action::ModifyPlayerVariable { variable, .. } => {
                Some((SymbolKind::PlayerVariable, variable, true))
            }
            Action::ForGlobalVariable { variable, .. } => {
                Some((SymbolKind::GlobalVariable, variable, false))
            }
            Action::ForPlayerVariable { variable, .. } => {
                Some((SymbolKind::PlayerVariable, variable, false))
            }
            _ => None,
        };
        if let Some((kind, name, reads_old_value)) = variable {
            if let Some(symbol) = self.find_symbol(kind, name) {
                self.push(
                    symbol,
                    ReferenceKind::Write,
                    Some(rule),
                    Some(action_id),
                    None,
                    action_occurrence(program, span, name),
                );
                if reads_old_value {
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
        }
        match action {
            Action::CallSubroutine { subroutine } => {
                if let Some(symbol) = self.find_symbol(SymbolKind::Subroutine, subroutine) {
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
            Action::Disabled { action } => {
                self.walk_action(action, rule, action_id, program);
                return;
            }
            _ => {}
        }
        visit_action_roots(action, &mut |argument, value| {
            self.walk_value(
                value,
                rule,
                Some(action_id),
                program.action_argument_span(rule, action_id, argument),
                program,
            );
        });
    }
    fn walk_value(
        &mut self,
        value: &Value,
        rule: RuleId,
        action: Option<ActionId>,
        span: Option<Span>,
        program: &Program,
    ) {
        visit_value_tree(value, None, &mut |value, _| {
            let value_id = self.next_value_id;
            self.next_value_id += 1;
            match value {
                Value::GlobalVariable(name) => {
                    if let Some(symbol) = self.find_symbol(SymbolKind::GlobalVariable, name) {
                        self.push(
                            symbol,
                            ReferenceKind::Read,
                            Some(rule),
                            action,
                            Some(value_id),
                            value_occurrence(program, span, name).or(span),
                        );
                    }
                }
                Value::PlayerVariable { variable, .. } => {
                    if let Some(symbol) = self.find_symbol(SymbolKind::PlayerVariable, variable) {
                        self.push(
                            symbol,
                            ReferenceKind::Read,
                            Some(rule),
                            action,
                            Some(value_id),
                            value_occurrence(program, span, variable).or(span),
                        );
                    }
                }
                _ => {}
            }
            value_id
        });
    }
}

fn declaration_span(program: &Program, prefix: &str, name: &str) -> Option<Span> {
    for file_index in 0..64 {
        let file = workshop_rs::source::FileId::from_index(file_index);
        let Some(source) = program.source(file) else {
            continue;
        };
        if let Some(span) =
            declaration_span_in_source(file, source.text(), std::slice::from_ref(&prefix), name)
        {
            return Some(span);
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
        if let Some(span) = declaration_span_in_source(*file, source, prefixes, name) {
            return Some(span);
        }
    }
    None
}

fn declaration_span_in_source(
    file: workshop_rs::source::FileId,
    source: &str,
    prefixes: &[&str],
    name: &str,
) -> Option<Span> {
    for (line_index, line) in source.lines().enumerate() {
        for prefix in prefixes {
            let Some(rest) = line.strip_prefix(prefix) else {
                continue;
            };
            let Some(found) = rest.split_whitespace().next() else {
                continue;
            };
            if found.trim_matches('"') != name {
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
    find_occurrence(source, span, name, before_assignment, ordinal, true)
}

fn find_occurrence(
    source: &str,
    span: Span,
    name: &str,
    before_assignment: bool,
    ordinal: usize,
    skip_non_code: bool,
) -> Option<Span> {
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
            if skip_non_code && !is_code_position(&chars, start) {
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
    Some(find_occurrence(source_doc.text(), span, name, true, 0, false).unwrap_or(span))
}

pub(super) fn value_occurrence(program: &Program, span: Option<Span>, name: &str) -> Option<Span> {
    let span = span?;
    let Some(source_doc) = program.source(span.file) else {
        return Some(span);
    };
    Some(find_occurrence(source_doc.text(), span, name, false, 0, false).unwrap_or(span))
}
