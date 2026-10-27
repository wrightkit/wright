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
    named: impl Iterator<Item = (SymbolKind, &'a str, usize)>,
) {
    for (kind, name, index) in named {
        let occurrence = match kind {
            SymbolKind::GlobalVariable => program.global_variable_name_span(index),
            SymbolKind::PlayerVariable => program.player_variable_name_span(index),
            SymbolKind::Subroutine => program.subroutine_name_span(index),
            SymbolKind::Rule => None,
        };
        let id = SymbolId::from_index(symbols.len());
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
    pub const ALL: [Self; 5] = [
        Self::Declaration,
        Self::Definition,
        Self::Read,
        Self::Write,
        Self::Call,
    ];

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

/// The outcome of resolving a 1-based source position to the symbol whose
/// identifier occurrence covers it (#429, #555). The editor surfaces —
/// hover, definition, and references — share this
/// one resolution rule, so a position resolves to exactly one symbol, to
/// none, or reports the ambiguity; no surface guesses by iteration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositionResolution<'a> {
    /// No symbol's identifier occurrence covers the position.
    Miss,
    /// Exactly one symbol covers the position; `occurrence` is the
    /// identifier span that matched.
    Resolved {
        symbol: &'a Symbol,
        occurrence: Span,
    },
    /// Distinct symbols' occurrences cover the position.
    Ambiguous(Vec<&'a Symbol>),
}

#[derive(Debug, Clone)]
pub struct SemanticIndex {
    symbols: Vec<Symbol>,
    references: Vec<Reference>,
    /// Positions in `references`, grouped by referenced symbol index.
    by_symbol: Vec<Vec<usize>>,
    next_value_id: ValueId,
}

pub(super) fn value_identity_map(program: &Program) -> HashMap<usize, ValueId> {
    let mut identities = HashMap::new();
    for rule in &program.rules {
        for condition in &rule.conditions {
            visit_value_tree(&condition.value, None, &mut |value, _, _| {
                let id = identities.len();
                identities.insert((value as *const Value) as usize, id);
                0
            });
        }
        for action in &rule.actions {
            visit_action_roots(action, &mut |_, root| {
                visit_value_tree(root, None, &mut |value, _, _| {
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
            program
                .global_variables
                .iter()
                .enumerate()
                .map(|(index, variable)| {
                    (SymbolKind::GlobalVariable, variable.name.as_str(), index)
                })
                .chain(
                    program
                        .player_variables
                        .iter()
                        .enumerate()
                        .map(|(index, variable)| {
                            (SymbolKind::PlayerVariable, variable.name.as_str(), index)
                        }),
                )
                .chain(
                    program
                        .subroutines
                        .iter()
                        .enumerate()
                        .map(|(index, subroutine)| {
                            (SymbolKind::Subroutine, subroutine.name.as_str(), index)
                        }),
                ),
        );
        for (rule, data) in program.rules.iter().enumerate() {
            let id = SymbolId::from_index(symbols.len());
            symbols.push(Symbol {
                id,
                kind: SymbolKind::Rule,
                name: data.name.clone(),
                span: program.rule_span(rule),
                occurrence: program
                    .rule_name_span(rule)
                    .or_else(|| program.rule_span(rule)),
                rule: Some(rule),
            });
        }
        let mut index = Self {
            symbols,
            references: Vec::new(),
            by_symbol: Vec::new(),
            next_value_id: 0,
        };
        index.add_missing_declarations();
        for (rule, data) in program.rules.iter().enumerate() {
            index.walk_event(&data.event, rule, program);
            for (condition, value) in data.conditions.iter().enumerate() {
                index.walk_value(
                    &value.value,
                    rule,
                    None,
                    ValueRoot::Condition(condition),
                    program,
                );
            }
            for (action, value) in data.actions.iter().enumerate() {
                index.walk_action(value, rule, action, program);
            }
        }
        let groups = index
            .references
            .iter()
            .map(|reference| reference.symbol.index() + 1)
            .max()
            .unwrap_or(0)
            .max(index.symbols.len());
        index.by_symbol = vec![Vec::new(); groups];
        for (position, reference) in index.references.iter().enumerate() {
            index.by_symbol[reference.symbol.index()].push(position);
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
        self.by_symbol
            .get(symbol.index())
            .map_or_else(Vec::new, |positions| {
                positions
                    .iter()
                    .map(|&position| &self.references[position])
                    .collect()
            })
    }

    /// Resolve a 1-based `line`/`col` to the symbol whose identifier
    /// occurrence covers it — the shared position→symbol rule. Every
    /// reference kind counts, including declarations, and several
    /// references of one symbol collapsing onto the same position still
    /// resolve that symbol once.
    pub fn symbol_at(&self, line: u32, col: u32) -> PositionResolution<'_> {
        let mut hits = Vec::new();
        for symbol in self.symbols() {
            let occurrence = self
                .references(symbol.id)
                .into_iter()
                .filter_map(|reference| reference.occurrence)
                .find(|occurrence| position_in_span(*occurrence, line, col));
            if let Some(occurrence) = occurrence {
                hits.push((symbol, occurrence));
            }
        }
        match hits.as_slice() {
            [] => PositionResolution::Miss,
            [(symbol, occurrence)] => PositionResolution::Resolved {
                symbol,
                occurrence: *occurrence,
            },
            _ => {
                PositionResolution::Ambiguous(hits.into_iter().map(|(symbol, _)| symbol).collect())
            }
        }
    }
    pub(super) fn references_for_all_symbols(&self) -> Vec<Vec<&Reference>> {
        self.symbols
            .iter()
            .map(|symbol| self.references(symbol.id))
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
        spans: (Option<Span>, Option<Span>),
    ) {
        self.references.push(Reference {
            symbol,
            kind,
            span: spans.0,
            occurrence: spans.1,
            rule,
            action,
            value,
        });
    }
    fn add_missing_declarations(&mut self) {
        let declared = self
            .references
            .iter()
            .filter(|reference| reference.kind == ReferenceKind::Declaration)
            .map(|reference| reference.symbol)
            .collect::<HashSet<_>>();
        self.references
            .extend(self.symbols.iter().filter_map(|symbol| {
                let span = symbol.occurrence?;
                (!declared.contains(&symbol.id)).then_some(Reference {
                    symbol: symbol.id,
                    kind: ReferenceKind::Declaration,
                    span: Some(span),
                    occurrence: Some(span),
                    rule: symbol.rule,
                    action: None,
                    value: None,
                })
            }));
    }
    fn find_symbol(&self, kind: SymbolKind, name: &str) -> Option<SymbolId> {
        self.symbols
            .iter()
            .rfind(|symbol| symbol.kind == kind && symbol.name == name)
            .map(|s| s.id)
    }
    fn walk_event(&mut self, event: &Event, rule: RuleId, program: &Program) {
        if let Event::Subroutine(name) = event
            && let Some(symbol) = self.find_symbol(SymbolKind::Subroutine, name)
        {
            let occurrence = program.rule_event_name_span(rule);
            self.push(
                symbol,
                ReferenceKind::Definition,
                Some(rule),
                None,
                None,
                (occurrence.or_else(|| program.rule_span(rule)), occurrence),
            );
        }
    }
    fn walk_action(
        &mut self,
        action: &Action,
        rule: RuleId,
        action_id: ActionId,
        program: &Program,
    ) {
        let identifier = program.action_identifier_span(rule, action_id);
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
        let statement = program.action_span(rule, action_id);
        if let Some((kind, name, reads_old_value)) = variable
            && let Some(symbol) = self.find_symbol(kind, name)
        {
            self.push(
                symbol,
                ReferenceKind::Write,
                Some(rule),
                Some(action_id),
                None,
                (identifier.or(statement), identifier),
            );
            if reads_old_value {
                self.push(
                    symbol,
                    ReferenceKind::Read,
                    Some(rule),
                    Some(action_id),
                    None,
                    (identifier.or(statement), identifier),
                );
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
                        (identifier.or(statement), identifier),
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
                ValueRoot::Argument {
                    action: action_id,
                    argument,
                },
                program,
            );
        });
    }
    fn walk_value(
        &mut self,
        value: &Value,
        rule: RuleId,
        action: Option<ActionId>,
        root: ValueRoot,
        program: &Program,
    ) {
        let statement = root.statement_span(program, rule);
        visit_value_tree(value, None, &mut |value, _, path| {
            let value_id = self.next_value_id;
            self.next_value_id += 1;
            let occurrence = root.span(program, rule, path);
            match value {
                Value::GlobalVariable(name) => {
                    if let Some(symbol) = self.find_symbol(SymbolKind::GlobalVariable, name) {
                        self.push(
                            symbol,
                            ReferenceKind::Read,
                            Some(rule),
                            action,
                            Some(value_id),
                            (occurrence.or(statement), occurrence),
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
                            (occurrence.or(statement), occurrence),
                        );
                    }
                }
                Value::Subroutine(name) => {
                    if let Some(symbol) = self.find_symbol(SymbolKind::Subroutine, name) {
                        self.push(
                            symbol,
                            ReferenceKind::Call,
                            Some(rule),
                            action,
                            Some(value_id),
                            (occurrence.or(statement), occurrence),
                        );
                    }
                }
                _ => {}
            }
            value_id
        });
    }
}

#[derive(Clone, Copy)]
enum ValueRoot {
    Condition(usize),
    Argument { action: usize, argument: usize },
}

impl ValueRoot {
    fn span(self, program: &Program, rule: usize, path: &[usize]) -> Option<Span> {
        match self {
            Self::Condition(condition) => program.condition_value_span(rule, condition, path),
            Self::Argument { action, argument } => {
                program.action_argument_value_span(rule, action, argument, path)
            }
        }
    }
    fn statement_span(self, program: &Program, rule: usize) -> Option<Span> {
        match self {
            Self::Condition(condition) => program.condition_span(rule, condition),
            Self::Argument { action, .. } => program.action_span(rule, action),
        }
    }
}

fn position_in_span(span: Span, line: u32, col: u32) -> bool {
    (line > span.start.line || (line == span.start.line && col >= span.start.col))
        && (line < span.end.line || (line == span.end.line && col < span.end.col))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(text: &str) -> (Program, SemanticIndex) {
        let catalog = workshop_rs::catalog::Catalog::builtin_ref().expect("builtin catalog");
        let program = workshop_rs::parser::parse_with_context(
            text,
            catalog,
            &workshop_rs::catalog::Locale::new("en-US"),
            catalog,
        )
        .expect("Workshop source must parse");
        let index = SemanticIndex::build(&program);
        (program, index)
    }

    #[test]
    fn symbol_at_resolves_each_symbol_kind_and_misses_outside_occurrences() {
        let text = concat!(
            "variables {\n",
            "    global:\n",
            "        0: score\n",
            "}\n",
            "\n",
            "subroutines {\n",
            "    0: shared\n",
            "}\n",
            "\n",
            "rule (\"main\") {\n",
            "    event {\n",
            "        Ongoing - Global;\n",
            "    }\n",
            "    actions {\n",
            "        Call Subroutine(shared);\n",
            "        Set Global Variable(score, 1);\n",
            "    }\n",
            "}\n",
        );
        let (_program, index) = index(text);
        let resolved = |line: u32, col: u32| match index.symbol_at(line, col) {
            PositionResolution::Resolved { symbol, occurrence } => {
                assert!(position_in_span(occurrence, line, col));
                (symbol.kind.as_str(), symbol.name.clone())
            }
            other => panic!("expected one symbol at {line}:{col}, got {other:?}"),
        };

        assert_eq!(resolved(3, 14), ("globalVariable", "score".to_string()));
        assert_eq!(resolved(16, 31), ("globalVariable", "score".to_string()));
        assert_eq!(resolved(15, 27), ("subroutine", "shared".to_string()));
        assert_eq!(resolved(10, 9), ("rule", "main".to_string()));

        for (line, col) in [(1, 1), (5, 1), (11, 2)] {
            assert!(
                matches!(index.symbol_at(line, col), PositionResolution::Miss),
                "no identifier occurrence covers {line}:{col}"
            );
        }
    }

    #[test]
    fn symbol_at_honors_half_open_occurrence_spans() {
        let text = concat!(
            "variables {\n",
            "    global:\n",
            "        0: score\n",
            "}\n",
        );
        let (_program, index) = index(text);
        assert!(matches!(
            index.symbol_at(3, 12),
            PositionResolution::Resolved { .. }
        ));
        assert!(matches!(index.symbol_at(3, 17), PositionResolution::Miss));
    }
}
