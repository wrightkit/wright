use std::collections::{HashMap, HashSet};

use workshop_rs::ids::Id;
use workshop_rs::source::{FileId, Position, Span};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    Declaration,
    Definition,
    Read,
    Write,
    Call,
}

pub type SymbolId = Id<Symbol>;

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
        for (idx, var) in program.global_variables.iter().enumerate() {
            let occ = declaration_span(program, &["globalvar "], &var.name);
            symbols.push(Symbol {
                id: SymbolId::from_index(idx),
                kind: SymbolKind::GlobalVariable,
                name: var.name.clone(),
                span: occ,
                occurrence: occ,
                rule: None,
            });
        }
        for var in &program.player_variables {
            let occ = declaration_span(program, &["playervar "], &var.name);
            symbols.push(Symbol {
                id: SymbolId::from_index(symbols.len()),
                kind: SymbolKind::PlayerVariable,
                name: var.name.clone(),
                span: occ,
                occurrence: occ,
                rule: None,
            });
        }
        for sub in &program.subroutines {
            let occ = declaration_span(program, &["subroutine "], &sub.name);
            symbols.push(Symbol {
                id: SymbolId::from_index(symbols.len()),
                kind: SymbolKind::Subroutine,
                name: sub.name.clone(),
                span: occ,
                occurrence: occ,
                rule: None,
            });
        }
        for (rule, data) in program.rules.iter().enumerate() {
            let span = program.rule_span(rule);
            symbols.push(Symbol {
                id: SymbolId::from_index(symbols.len()),
                kind: SymbolKind::Rule,
                name: data.name.clone(),
                span,
                occurrence: span,
                rule: Some(rule),
            });
        }

        let mut index = Self {
            symbols,
            references: Vec::new(),
        };
        for symbol in index.symbols.clone() {
            if let Some(span) = symbol.occurrence {
                index.push(symbol.id, ReferenceKind::Declaration, symbol.rule, None, None, Some(span));
            }
        }
        for (rule, data) in program.rules.iter().enumerate() {
            index.walk_event(&data.event, rule, program);
            for (cond_idx, cond) in data.conditions.iter().enumerate() {
                index.walk_value(&cond.value, rule, None, Some(cond_idx), program.condition_span(rule, cond_idx), program);
            }
            for (act_idx, action) in data.actions.iter().enumerate() {
                index.walk_action(action, rule, act_idx, program);
            }
        }
        index
    }

    pub fn build_with_sources(
        program: &Program,
        sources: &[(FileId, String)],
    ) -> Self {
        let mut index = Self::build(program);
        for symbol in &mut index.symbols {
            let prefixes: &[&str] = match symbol.kind {
                SymbolKind::GlobalVariable => &["globalvar "],
                SymbolKind::PlayerVariable => &["playervar "],
                SymbolKind::Subroutine => &["subroutine "],
                SymbolKind::Rule => &[],
            };
            if let Some(span) = declaration_span_in_sources(sources, prefixes, &symbol.name) {
                symbol.span = Some(declaration_line_span(sources, span));
                symbol.occurrence = Some(span);
            }
        }
        for symbol in index.symbols.clone() {
            if let Some(span) = symbol.occurrence {
                if !index.references.iter().any(|r| r.symbol == symbol.id && r.kind == ReferenceKind::Declaration) {
                    index.push(symbol.id, ReferenceKind::Declaration, symbol.rule, None, None, Some(span));
                }
            }
        }
        let symbols = index.symbols.clone();
        let mut read_occurrences = HashMap::new();
        for reference in &mut index.references {
            let symbol = &symbols[reference.symbol.index()];
            let span = match reference.kind {
                ReferenceKind::Declaration => symbol.occurrence,
                ReferenceKind::Definition => reference
                    .rule
                    .and_then(|r| program.rule_span(r))
                    .and_then(|span| occurrence_in_sources(sources, span, &symbol.name, false, 0)),
                ReferenceKind::Write | ReferenceKind::Call => reference
                    .rule
                    .and_then(|r| reference.action.and_then(|a| program.action_span(r, a)))
                    .and_then(|span| occurrence_in_sources(sources, span, &symbol.name, true, 0)),
                ReferenceKind::Read => {
                    let key = (reference.symbol, reference.rule, reference.action, reference.value);
                    let ordinal = read_occurrences.entry(key).or_insert(0);
                    let action_span = reference.rule.and_then(|r| reference.action.and_then(|a| program.action_span(r, a)));
                    let implicit_modify = reference.value.is_none() && reference.span.is_some() && reference.span == action_span;
                    let current = if implicit_modify { 1 } else { *ordinal };
                    *ordinal += 1;
                    reference.span
                        .or_else(|| action_span.or_else(|| reference.rule.and_then(|r| reference.value.and_then(|v| program.condition_span(r, v)))))
                        .and_then(|span| occurrence_in_sources(sources, span, &symbol.name, false, current))
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
        self.references.iter().filter(|r| r.symbol == symbol).collect()
    }

    pub fn usage(&self, symbol: SymbolId) -> UsageSummary {
        let mut usage = UsageSummary::default();
        let mut rules = HashSet::new();
        for r in self.references(symbol) {
            match r.kind {
                ReferenceKind::Read => usage.reads += 1,
                ReferenceKind::Write => usage.writes += 1,
                ReferenceKind::Call => usage.calls += 1,
                _ => {}
            }
            if let Some(rule) = r.rule {
                rules.insert(rule);
            }
        }
        usage.rules = rules.len() as u32;
        usage
    }

    fn push(&mut self, symbol: SymbolId, kind: ReferenceKind, rule: Option<RuleId>, action: Option<ActionId>, value: Option<ValueId>, span: Option<Span>) {
        self.references.push(Reference { symbol, kind, span, occurrence: span, rule, action, value });
    }

    fn global(&self, name: &str) -> Option<SymbolId> {
        self.symbols.iter().find(|s| s.kind == SymbolKind::GlobalVariable && s.name == name).map(|s| s.id)
    }

    fn player(&self, name: &str) -> Option<SymbolId> {
        self.symbols.iter().find(|s| s.kind == SymbolKind::PlayerVariable && s.name == name).map(|s| s.id)
    }

    fn subroutine(&self, name: &str) -> Option<SymbolId> {
        self.symbols.iter().find(|s| s.kind == SymbolKind::Subroutine && s.name == name).map(|s| s.id)
    }

    fn walk_event(&mut self, event: &Event, rule: RuleId, program: &Program) {
        if let Event::Subroutine(name) = event {
            if let Some(id) = self.subroutine(name) {
                let span = program.rule_span(rule);
                self.push(id, ReferenceKind::Definition, Some(rule), None, None, span);
            }
        }
    }

    fn walk_action(&mut self, action: &Action, rule: RuleId, action_idx: usize, program: &Program) {
        let action_span = program.action_span(rule, action_idx);
        match action {
            Action::SetGlobalVariable { variable, value } => {
                if let Some(id) = self.global(variable) {
                    let occ = action_occurrence(program, action_span, variable).or(action_span);
                    self.push(id, ReferenceKind::Write, Some(rule), Some(action_idx), None, occ);
                }
                self.walk_value(value, rule, Some(action_idx), None, action_span, program);
            }
            Action::ModifyGlobalVariable { variable, value, .. } => {
                if let Some(id) = self.global(variable) {
                    let occ = action_occurrence(program, action_span, variable).or(action_span);
                    self.push(id, ReferenceKind::Write, Some(rule), Some(action_idx), None, occ);
                    self.push(id, ReferenceKind::Read, Some(rule), Some(action_idx), None, occ);
                }
                self.walk_value(value, rule, Some(action_idx), None, action_span, program);
            }
            Action::SetPlayerVariable { player, variable, value } => {
                if let Some(id) = self.player(variable) {
                    let occ = action_occurrence(program, action_span, variable).or(action_span);
                    self.push(id, ReferenceKind::Write, Some(rule), Some(action_idx), None, occ);
                }
                self.walk_value(player, rule, Some(action_idx), None, action_span, program);
                self.walk_value(value, rule, Some(action_idx), None, action_span, program);
            }
            Action::ModifyPlayerVariable { player, variable, value, .. } => {
                if let Some(id) = self.player(variable) {
                    let occ = action_occurrence(program, action_span, variable).or(action_span);
                    self.push(id, ReferenceKind::Write, Some(rule), Some(action_idx), None, occ);
                    self.push(id, ReferenceKind::Read, Some(rule), Some(action_idx), None, occ);
                }
                self.walk_value(player, rule, Some(action_idx), None, action_span, program);
                self.walk_value(value, rule, Some(action_idx), None, action_span, program);
            }
            Action::CallSubroutine { subroutine } => {
                if let Some(id) = self.subroutine(subroutine) {
                    let occ = action_occurrence(program, action_span, subroutine).or(action_span);
                    self.push(id, ReferenceKind::Call, Some(rule), Some(action_idx), None, occ);
                }
            }
            Action::Call { name, args } => {
                if let Some(id) = self.subroutine(name) {
                    let occ = action_occurrence(program, action_span, name).or(action_span);
                    self.push(id, ReferenceKind::Call, Some(rule), Some(action_idx), None, occ);
                }
                for arg in args {
                    self.walk_value(arg, rule, Some(action_idx), None, action_span, program);
                }
            }
            Action::If { condition } | Action::ElseIf { condition } | Action::While { condition } => {
                self.walk_value(condition, rule, Some(action_idx), None, action_span, program);
            }
            Action::ForGlobalVariable { variable, start, stop, step } => {
                if let Some(id) = self.global(variable) {
                    let occ = action_occurrence(program, action_span, variable);
                    self.push(id, ReferenceKind::Write, Some(rule), Some(action_idx), None, occ);
                }
                self.walk_value(start, rule, Some(action_idx), None, action_span, program);
                self.walk_value(stop, rule, Some(action_idx), None, action_span, program);
                self.walk_value(step, rule, Some(action_idx), None, action_span, program);
            }
            Action::ForPlayerVariable { player, variable, start, stop, step } => {
                if let Some(id) = self.player(variable) {
                    let occ = action_occurrence(program, action_span, variable);
                    self.push(id, ReferenceKind::Write, Some(rule), Some(action_idx), None, occ);
                }
                self.walk_value(player, rule, Some(action_idx), None, action_span, program);
                self.walk_value(start, rule, Some(action_idx), None, action_span, program);
                self.walk_value(stop, rule, Some(action_idx), None, action_span, program);
                self.walk_value(step, rule, Some(action_idx), None, action_span, program);
            }
            Action::AssignMember { target, value, .. } => {
                self.walk_value(target, rule, Some(action_idx), None, action_span, program);
                self.walk_value(value, rule, Some(action_idx), None, action_span, program);
            }
            Action::Disabled { action } => self.walk_action(action, rule, action_idx, program),
            Action::Else | Action::End => {}
        }
    }

    fn walk_value(&mut self, value: &Value, rule: RuleId, action: Option<ActionId>, condition: Option<ValueId>, span: Option<Span>, program: &Program) {
        match value {
            Value::GlobalVariable(name) => {
                if let Some(id) = self.global(name) {
                    let occ = value_occurrence(program, span, name);
                    self.push(id, ReferenceKind::Read, Some(rule), action, condition, occ);
                }
            }
            Value::PlayerVariable { player, variable } => {
                if let Some(id) = self.player(variable) {
                    let occ = value_occurrence(program, span, variable);
                    self.push(id, ReferenceKind::Read, Some(rule), action, condition, occ);
                }
                self.walk_value(player, rule, action, condition, span, program);
            }
            Value::Subroutine(name) => {
                if let Some(id) = self.subroutine(name) {
                    let occ = value_occurrence(program, span, name);
                    self.push(id, ReferenceKind::Call, Some(rule), action, condition, occ);
                }
            }
            Value::Array(elements) => {
                for el in elements {
                    self.walk_value(el, rule, action, condition, span, program);
                }
            }
            Value::Vector { x, y, z } => {
                self.walk_value(x, rule, action, condition, span, program);
                self.walk_value(y, rule, action, condition, span, program);
                self.walk_value(z, rule, action, condition, span, program);
            }
            Value::Call { name, args } => {
                if let Some(id) = self.subroutine(name) {
                    let occ = value_occurrence(program, span, name);
                    self.push(id, ReferenceKind::Call, Some(rule), action, condition, occ);
                }
                for arg in args {
                    self.walk_value(arg, rule, action, condition, span, program);
                }
            }
            _ => {}
        }
    }
}

fn declaration_span(program: &Program, prefixes: &[&str], name: &str) -> Option<Span> {
    for file_index in 0..64 {
        let file = FileId::from_index(file_index);
        let Some(source) = program.source(file) else {
            continue;
        };
        let text = source.text();
        let name_chars: Vec<char> = name.chars().collect();
        for (line_idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if !prefixes.is_empty() && !prefixes.iter().any(|p| trimmed.starts_with(p)) {
                continue;
            }
            let chars: Vec<char> = line.chars().collect();
            for start in 0..=chars.len().saturating_sub(name_chars.len()) {
                let end = start + name_chars.len();
                if &chars[start..end] != name_chars.as_slice() {
                    continue;
                }
                let before = start.checked_sub(1).and_then(|i| chars.get(i));
                let after = chars.get(end);
                if before.is_some_and(|c| c.is_alphanumeric() || *c == '_') || after.is_some_and(|c| c.is_alphanumeric() || *c == '_') {
                    continue;
                }
                return Some(Span::new(
                    file,
                    Position::new((line_idx + 1) as u32, (start + 1) as u32),
                    Position::new((line_idx + 1) as u32, (end + 1) as u32),
                ));
            }
        }
    }
    None
}

fn declaration_span_in_sources(sources: &[(FileId, String)], prefixes: &[&str], name: &str) -> Option<Span> {
    for (file_id, text) in sources {
        let name_chars: Vec<char> = name.chars().collect();
        for (line_idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if !prefixes.is_empty() && !prefixes.iter().any(|p| trimmed.starts_with(p)) {
                continue;
            }
            let chars: Vec<char> = line.chars().collect();
            for start in 0..=chars.len().saturating_sub(name_chars.len()) {
                let end = start + name_chars.len();
                if &chars[start..end] != name_chars.as_slice() {
                    continue;
                }
                let before = start.checked_sub(1).and_then(|i| chars.get(i));
                let after = chars.get(end);
                if before.is_some_and(|c| c.is_alphanumeric() || *c == '_') || after.is_some_and(|c| c.is_alphanumeric() || *c == '_') {
                    continue;
                }
                return Some(Span::new(
                    *file_id,
                    Position::new((line_idx + 1) as u32, (start + 1) as u32),
                    Position::new((line_idx + 1) as u32, (end + 1) as u32),
                ));
            }
        }
    }
    None
}

fn declaration_line_span(sources: &[(FileId, String)], span: Span) -> Span {
    let Some((_, text)) = sources.iter().find(|(id, _)| *id == span.file) else {
        return span;
    };
    let line_number = span.start.line as usize;
    let Some(line) = text.lines().nth(line_number.saturating_sub(1)) else {
        return span;
    };
    let start_col = line.chars().take_while(|c| c.is_whitespace()).count() + 1;
    let end_col = line.trim_end().chars().count() + 1;
    Span::new(
        span.file,
        Position::new(span.start.line, start_col as u32),
        Position::new(span.start.line, end_col.max(start_col) as u32),
    )
}

fn occurrence_in_sources(sources: &[(FileId, String)], span: Span, name: &str, limit_to_assignment_target: bool, ordinal: usize) -> Option<Span> {
    let (_, text) = sources.iter().find(|(id, _)| *id == span.file)?;
    let name_chars: Vec<char> = name.chars().collect();
    let mut found = 0;
    for line_num in span.start.line..=span.end.line {
        let line = text.lines().nth(line_num.saturating_sub(1) as usize)?;
        let chars: Vec<char> = line.chars().collect();
        let lower = if line_num == span.start.line { span.start.col.saturating_sub(1) as usize } else { 0 };
        let mut upper = if line_num == span.end.line { span.end.col.saturating_sub(1) as usize } else { chars.len() };
        if limit_to_assignment_target {
            if let Some(op) = line.find('=') {
                upper = upper.min(op);
            }
        }
        for start in lower.min(chars.len())..=upper.min(chars.len()) {
            let end = start.saturating_add(name_chars.len());
            if end > upper || chars.get(start..end) != Some(name_chars.as_slice()) {
                continue;
            }
            let before = start.checked_sub(1).and_then(|i| chars.get(i));
            let after = chars.get(end);
            if before.is_some_and(|c| c.is_alphanumeric() || *c == '_') || after.is_some_and(|c| c.is_alphanumeric() || *c == '_') {
                continue;
            }
            if !is_code_position(&chars, start) {
                continue;
            }
            if found == ordinal {
                return Some(Span::new(
                    span.file,
                    Position::new(line_num, start as u32 + 1),
                    Position::new(line_num, end as u32 + 1),
                ));
            }
            found += 1;
        }
    }
    None
}

fn is_code_position(chars: &[char], position: usize) -> bool {
    let mut quoted = false;
    let mut escaped = false;
    for ch in chars.iter().take(position) {
        if *ch == '#' && !quoted {
            return false;
        }
        if *ch == '"' && !escaped {
            quoted = !quoted;
        }
        escaped = *ch == '\\' && !escaped;
        if *ch != '\\' {
            escaped = false;
        }
    }
    !quoted
}

fn action_occurrence(program: &Program, span: Option<Span>, name: &str) -> Option<Span> {
    let span = span?;
    let source = program.source(span.file)?.text();
    let name_chars: Vec<char> = name.chars().collect();
    for line_num in span.start.line..=span.end.line {
        let line = source.lines().nth(line_num.saturating_sub(1) as usize)?;
        let chars: Vec<char> = line.chars().collect();
        let lower = if line_num == span.start.line { span.start.col.saturating_sub(1) as usize } else { 0 };
        let mut upper = if line_num == span.end.line { span.end.col.saturating_sub(1) as usize } else { chars.len() };
        if let Some(op) = line.find('=') {
            upper = upper.min(op);
        }
        for start in lower.min(chars.len())..=upper.min(chars.len()) {
            let end = start.saturating_add(name_chars.len());
            if end > upper || chars.get(start..end) != Some(name_chars.as_slice()) {
                continue;
            }
            let before = start.checked_sub(1).and_then(|i| chars.get(i));
            let after = chars.get(end);
            if before.is_some_and(|c| c.is_alphanumeric() || *c == '_') || after.is_some_and(|c| c.is_alphanumeric() || *c == '_') {
                continue;
            }
            return Some(Span::new(
                span.file,
                Position::new(line_num, start as u32 + 1),
                Position::new(line_num, end as u32 + 1),
            ));
        }
    }
    None
}

fn value_occurrence(program: &Program, span: Option<Span>, name: &str) -> Option<Span> {
    let span = span?;
    let source = program.source(span.file)?.text();
    let name_chars: Vec<char> = name.chars().collect();
    for line_num in span.start.line..=span.end.line {
        let line = source.lines().nth(line_num.saturating_sub(1) as usize)?;
        let chars: Vec<char> = line.chars().collect();
        let lower = if line_num == span.start.line { span.start.col.saturating_sub(1) as usize } else { 0 };
        let upper = if line_num == span.end.line { span.end.col.saturating_sub(1) as usize } else { chars.len() };
        for start in lower.min(chars.len())..=upper.min(chars.len()) {
            let end = start.saturating_add(name_chars.len());
            if end > upper || chars.get(start..end) != Some(name_chars.as_slice()) {
                continue;
            }
            let before = start.checked_sub(1).and_then(|i| chars.get(i));
            let after = chars.get(end);
            if before.is_some_and(|c| c.is_alphanumeric() || *c == '_') || after.is_some_and(|c| c.is_alphanumeric() || *c == '_') {
                continue;
            }
            return Some(Span::new(
                span.file,
                Position::new(line_num, start as u32 + 1),
                Position::new(line_num, end as u32 + 1),
            ));
        }
    }
    None
}
