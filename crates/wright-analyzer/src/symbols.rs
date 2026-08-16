//! Symbol tables, reference indices, and usage queries over Workshop IR.
//!
//! [`SemanticIndex`] is the read-only semantic query surface for tooling and
//! agents: it enumerates every symbol (global/player variables, subroutines,
//! rules), records every reference site (declarations, reads, writes, calls,
//! definitions) with its source span and rule/action/value context, and
//! answers usage and find-references queries without scraping source text.

use std::collections::HashSet;

use workshop_rs::source::Span;
use workshop_rs::wir::{
    self, Action, ActionId, GlobalVarId, PlayerVarId, RuleId, SubroutineId, Value, ValueId,
};
use wright_ir::arena::Arena;
use wright_ir::error::IrError;
use wright_ir::ids::Id;

/// A typed ID referencing a [`Symbol`].
pub type SymbolId = Id<Symbol>;

/// The kind of a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    GlobalVariable,
    PlayerVariable,
    Subroutine,
    Rule,
}

/// A declared symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub id: SymbolId,
    pub kind: SymbolKind,
    pub name: String,
    /// The declaration site, when the IR carries one.
    pub span: Option<Span>,
    /// The exact span of the declared identifier occurrence (the rename
    /// target); `None` when provenance could not be preserved.
    pub occurrence: Option<Span>,
    /// The rule that owns this symbol, for rule symbols.
    pub rule: Option<RuleId>,
}

/// The kind of a reference to a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// The symbol's own declaration site.
    Declaration,
    /// A subroutine definition event (`def` body rule).
    Definition,
    /// A read of a variable value.
    Read,
    /// A write or bind to a variable (set, modify, for-loop binding).
    Write,
    /// A subroutine call.
    Call,
}

/// One reference to a symbol, with its source location and IR context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub symbol: SymbolId,
    pub kind: ReferenceKind,
    pub span: Option<Span>,
    /// The exact span of the identifier occurrence (the rename target);
    /// `None` when provenance could not be preserved.
    pub occurrence: Option<Span>,
    /// The containing rule, for references inside rules.
    pub rule: Option<RuleId>,
    /// The containing action, for references inside actions.
    pub action: Option<ActionId>,
    /// The value node the reference occurs in, for value reads.
    pub value: Option<ValueId>,
}

/// Aggregate usage counts for one symbol.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageSummary {
    pub reads: u32,
    pub writes: u32,
    pub calls: u32,
    /// Number of distinct rules referencing the symbol.
    pub rules: u32,
}

/// The semantic index over one Workshop IR program.
#[derive(Debug, Clone)]
pub struct SemanticIndex {
    symbols: Arena<Symbol>,
    references: Vec<Reference>,
    references_by_symbol: Vec<Vec<usize>>,
}

impl SemanticIndex {
    /// Build the index by walking the program's declarations, rules, actions,
    /// and values in deterministic order.
    pub fn build(program: &wir::Program) -> Result<SemanticIndex, IrError> {
        Builder::new(program).build()
    }

    /// All symbols in declaration order.
    pub fn symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.iter()
    }

    /// The symbol with the given ID, if any.
    pub fn symbol(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(id)
    }

    /// Every symbol of the given kind, in declaration order.
    pub fn symbols_of(&self, kind: SymbolKind) -> Vec<&Symbol> {
        self.symbols
            .iter()
            .filter(|symbol| symbol.kind == kind)
            .collect()
    }

    /// Symbol IDs whose name matches exactly.
    pub fn find_by_name(&self, name: &str) -> Vec<SymbolId> {
        self.symbols
            .iter()
            .filter(|symbol| symbol.name == name)
            .map(|symbol| symbol.id)
            .collect()
    }

    /// Every reference to the given symbol, in program order.
    pub fn references(&self, symbol: SymbolId) -> Vec<&Reference> {
        let Some(slots) = self.references_by_symbol.get(symbol.index()) else {
            return Vec::new();
        };
        slots
            .iter()
            .filter_map(|index| self.references.get(*index))
            .collect()
    }

    /// Aggregate usage counts for the given symbol.
    pub fn usage(&self, symbol: SymbolId) -> UsageSummary {
        let mut summary = UsageSummary::default();
        let mut rules = HashSet::new();
        for reference in self.references(symbol) {
            match reference.kind {
                ReferenceKind::Read => summary.reads += 1,
                ReferenceKind::Write => summary.writes += 1,
                ReferenceKind::Call => summary.calls += 1,
                ReferenceKind::Declaration | ReferenceKind::Definition => {}
            }
            if let Some(rule) = reference.rule {
                rules.insert(rule);
            }
        }
        summary.rules = rules.len() as u32;
        summary
    }
}

struct Builder<'a> {
    program: &'a wir::Program,
    symbols: Arena<Symbol>,
    references: Vec<Reference>,
    references_by_symbol: Vec<Vec<usize>>,
}

impl<'a> Builder<'a> {
    fn new(program: &'a wir::Program) -> Self {
        Builder {
            program,
            symbols: Arena::new(),
            references: Vec::new(),
            references_by_symbol: Vec::new(),
        }
    }

    fn build(mut self) -> Result<SemanticIndex, IrError> {
        // Symbol tables, in the fixed order the id arithmetic relies on:
        // globals, players, subroutines, rules.
        for id in 0..self.program.global_variables.len() {
            let variable = self
                .program
                .global_variables
                .get(GlobalVarId::from_index(id))
                .ok_or_else(|| dangling("global variable", id))?;
            let symbol = self.symbols.push(Symbol {
                id: SymbolId::from_index(id),
                kind: SymbolKind::GlobalVariable,
                name: variable.name.clone(),
                span: variable.span,
                occurrence: variable.name_span,
                rule: None,
            });
            self.declare(symbol, variable.span, variable.name_span, None)?;
        }
        let global_count = self.symbols.len();
        for id in 0..self.program.player_variables.len() {
            let variable = self
                .program
                .player_variables
                .get(PlayerVarId::from_index(id))
                .ok_or_else(|| dangling("player variable", id))?;
            let symbol = self.symbols.push(Symbol {
                id: SymbolId::from_index(global_count + id),
                kind: SymbolKind::PlayerVariable,
                name: variable.name.clone(),
                span: variable.span,
                occurrence: variable.name_span,
                rule: None,
            });
            self.declare(symbol, variable.span, variable.name_span, None)?;
        }
        let player_start = self.symbols.len();
        for id in 0..self.program.subroutines.len() {
            let subroutine = self
                .program
                .subroutines
                .get(SubroutineId::from_index(id))
                .ok_or_else(|| dangling("subroutine", id))?;
            let symbol = self.symbols.push(Symbol {
                id: SymbolId::from_index(player_start + id),
                kind: SymbolKind::Subroutine,
                name: subroutine.name.clone(),
                span: subroutine.span,
                occurrence: subroutine.name_span,
                rule: None,
            });
            self.declare(symbol, subroutine.span, subroutine.name_span, None)?;
        }
        let subroutine_start = self.symbols.len();
        for id in 0..self.program.rules.len() {
            let rule = self
                .program
                .rules
                .get(RuleId::from_index(id))
                .ok_or_else(|| dangling("rule", id))?;
            let symbol = self.symbols.push(Symbol {
                id: SymbolId::from_index(subroutine_start + id),
                kind: SymbolKind::Rule,
                name: rule.name.clone(),
                span: rule.span,
                occurrence: rule.name_span,
                rule: Some(RuleId::from_index(id)),
            });
            self.declare(
                symbol,
                rule.span,
                rule.name_span,
                Some(RuleId::from_index(id)),
            )?;
        }

        // References from rule bodies.
        for id in 0..self.program.rules.len() {
            self.walk_rule(RuleId::from_index(id))?;
        }
        Ok(SemanticIndex {
            symbols: self.symbols,
            references: self.references,
            references_by_symbol: self.references_by_symbol,
        })
    }

    fn declare(
        &mut self,
        symbol: SymbolId,
        span: Option<Span>,
        occurrence: Option<Span>,
        rule: Option<RuleId>,
    ) -> Result<(), IrError> {
        self.push(Reference {
            symbol,
            kind: ReferenceKind::Declaration,
            span,
            occurrence,
            rule,
            action: None,
            value: None,
        })
    }

    fn walk_rule(&mut self, rule: RuleId) -> Result<(), IrError> {
        let rule_data = self
            .program
            .rules
            .get(rule)
            .ok_or_else(|| dangling("rule", rule.index()))?
            .clone();
        for condition in &rule_data.conditions {
            self.walk_value(*condition, Some(rule), None)?;
        }
        for action in &rule_data.actions {
            self.walk_action(*action, Some(rule))?;
        }
        if let wir::Event::Subroutine(subroutine) = &rule_data.event {
            let symbol = self.subroutine_symbol(*subroutine)?;
            self.push(Reference {
                symbol,
                kind: ReferenceKind::Definition,
                span: rule_data.span,
                occurrence: rule_data.name_span,
                rule: Some(rule),
                action: None,
                value: None,
            })?;
        }
        Ok(())
    }

    fn walk_action(&mut self, action_id: ActionId, rule: Option<RuleId>) -> Result<(), IrError> {
        let action = self
            .program
            .actions
            .get(action_id)
            .ok_or_else(|| dangling("action", action_id.index()))?
            .clone();
        let span = action.span();
        match &action {
            Action::SetGlobalVariable {
                variable,
                value,
                target_span,
                ..
            } => {
                self.write(
                    self.global_symbol(*variable)?,
                    span,
                    *target_span,
                    rule,
                    Some(action_id),
                )?;
                self.walk_value(*value, rule, Some(action_id))
            }
            Action::ModifyGlobalVariable {
                variable,
                value,
                target_span,
                ..
            } => {
                // A modify reads and writes; it is indexed as a write.
                self.write(
                    self.global_symbol(*variable)?,
                    span,
                    *target_span,
                    rule,
                    Some(action_id),
                )?;
                self.walk_value(*value, rule, Some(action_id))
            }
            Action::SetPlayerVariable {
                player,
                variable,
                value,
                target_span,
                ..
            } => {
                self.write(
                    self.player_symbol(*variable)?,
                    span,
                    *target_span,
                    rule,
                    Some(action_id),
                )?;
                self.walk_value(*player, rule, Some(action_id))?;
                self.walk_value(*value, rule, Some(action_id))
            }
            Action::ModifyPlayerVariable {
                player,
                variable,
                value,
                target_span,
                ..
            } => {
                self.write(
                    self.player_symbol(*variable)?,
                    span,
                    *target_span,
                    rule,
                    Some(action_id),
                )?;
                self.walk_value(*player, rule, Some(action_id))?;
                self.walk_value(*value, rule, Some(action_id))
            }
            Action::CallSubroutine {
                subroutine,
                callee_span,
                ..
            } => self.call(
                self.subroutine_symbol(*subroutine)?,
                span,
                *callee_span,
                rule,
                Some(action_id),
            ),
            Action::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    self.walk_value(branch.condition, rule, Some(action_id))?;
                    for action in &branch.body {
                        self.walk_action(*action, rule)?;
                    }
                }
                if let Some(else_body) = else_body {
                    for action in else_body {
                        self.walk_action(*action, rule)?;
                    }
                }
                Ok(())
            }
            Action::While {
                condition, body, ..
            } => {
                self.walk_value(*condition, rule, Some(action_id))?;
                for action in body {
                    self.walk_action(*action, rule)?;
                }
                Ok(())
            }
            Action::ForGlobalVariable {
                variable,
                start,
                stop,
                step,
                body,
                target_span,
                ..
            } => {
                self.write(
                    self.global_symbol(*variable)?,
                    span,
                    *target_span,
                    rule,
                    Some(action_id),
                )?;
                self.walk_value(*start, rule, Some(action_id))?;
                self.walk_value(*stop, rule, Some(action_id))?;
                self.walk_value(*step, rule, Some(action_id))?;
                for action in body {
                    self.walk_action(*action, rule)?;
                }
                Ok(())
            }
            Action::ForPlayerVariable {
                variable,
                start,
                stop,
                step,
                body,
                ..
            } => {
                self.write(
                    self.player_symbol(*variable)?,
                    span,
                    None,
                    rule,
                    Some(action_id),
                )?;
                self.walk_value(*start, rule, Some(action_id))?;
                self.walk_value(*stop, rule, Some(action_id))?;
                self.walk_value(*step, rule, Some(action_id))?;
                for action in body {
                    self.walk_action(*action, rule)?;
                }
                Ok(())
            }
            Action::Debug { value, .. } => self.walk_value(*value, rule, Some(action_id)),
            Action::Print { message, .. } => self.walk_value(*message, rule, Some(action_id)),
            Action::Call { args, .. } => {
                for arg in args {
                    self.walk_value(*arg, rule, Some(action_id))?;
                }
                Ok(())
            }
        }
    }

    fn walk_value(
        &mut self,
        value_id: ValueId,
        rule: Option<RuleId>,
        action: Option<ActionId>,
    ) -> Result<(), IrError> {
        let node = self
            .program
            .values
            .get(value_id)
            .ok_or_else(|| dangling("value", value_id.index()))?;
        match &node.value {
            Value::GlobalVariable(variable) => self.read(
                self.global_symbol(*variable)?,
                node.span,
                // A global-variable value node spans exactly its name token.
                node.span,
                rule,
                action,
                Some(value_id),
            ),
            Value::PlayerVariable { player, variable } => {
                // A player-variable value node spans the whole
                // `receiver.member` expression; the member name is its final
                // token, derived here where the identity is known.
                let occurrence = self.player_occurrence(*variable, node.span);
                self.read(
                    self.player_symbol(*variable)?,
                    node.span,
                    occurrence,
                    rule,
                    action,
                    Some(value_id),
                )?;
                self.walk_value(*player, rule, action)
            }
            Value::Array(elements) => {
                for element in elements {
                    self.walk_value(*element, rule, action)?;
                }
                Ok(())
            }
            Value::Vector { x, y, z } => {
                self.walk_value(*x, rule, action)?;
                self.walk_value(*y, rule, action)?;
                self.walk_value(*z, rule, action)
            }
            Value::Call { args, .. } => {
                for arg in args {
                    self.walk_value(*arg, rule, action)?;
                }
                Ok(())
            }
            Value::Number { .. }
            | Value::String(_)
            | Value::Bool(_)
            | Value::Null
            | Value::Enum { .. }
            | Value::EventPlayer => Ok(()),
        }
    }

    fn read(
        &mut self,
        symbol: SymbolId,
        span: Option<Span>,
        occurrence: Option<Span>,
        rule: Option<RuleId>,
        action: Option<ActionId>,
        value: Option<ValueId>,
    ) -> Result<(), IrError> {
        self.push(Reference {
            symbol,
            kind: ReferenceKind::Read,
            span,
            occurrence,
            rule,
            action,
            value,
        })
    }

    fn write(
        &mut self,
        symbol: SymbolId,
        span: Option<Span>,
        occurrence: Option<Span>,
        rule: Option<RuleId>,
        action: Option<ActionId>,
    ) -> Result<(), IrError> {
        self.push(Reference {
            symbol,
            kind: ReferenceKind::Write,
            span,
            occurrence,
            rule,
            action,
            value: None,
        })
    }

    fn call(
        &mut self,
        symbol: SymbolId,
        span: Option<Span>,
        occurrence: Option<Span>,
        rule: Option<RuleId>,
        action: Option<ActionId>,
    ) -> Result<(), IrError> {
        self.push(Reference {
            symbol,
            kind: ReferenceKind::Call,
            span,
            occurrence,
            rule,
            action,
            value: None,
        })
    }

    /// The exact member-name occurrence of a player variable inside a
    /// `receiver.member` span: the member token is the final token of the
    /// member expression, so the occurrence ends at the span end and starts
    /// `name` characters earlier (columns are char-based).
    fn player_occurrence(&self, variable: PlayerVarId, span: Option<Span>) -> Option<Span> {
        let span = span?;
        let name_len = self
            .program
            .player_variables
            .get(variable)
            .map(|player| player.name.chars().count() as u32)
            .unwrap_or(0);
        Some(Span::new(
            span.file,
            workshop_rs::source::Position::new(
                span.end.line,
                span.end.col.saturating_sub(name_len).max(span.start.col),
            ),
            span.end,
        ))
    }

    fn push(&mut self, reference: Reference) -> Result<(), IrError> {
        let index = self.references.len();
        self.references.push(reference);
        let symbol_index = self.references[index].symbol.index();
        if self.references_by_symbol.len() <= symbol_index {
            self.references_by_symbol
                .resize(symbol_index + 1, Vec::new());
        }
        self.references_by_symbol[symbol_index].push(index);
        Ok(())
    }

    fn global_symbol(&self, id: GlobalVarId) -> Result<SymbolId, IrError> {
        if self.program.global_variables.contains(id) {
            Ok(SymbolId::from_index(id.index()))
        } else {
            Err(dangling("global variable", id.index()))
        }
    }

    fn player_symbol(&self, id: PlayerVarId) -> Result<SymbolId, IrError> {
        if self.program.player_variables.contains(id) {
            Ok(SymbolId::from_index(
                self.program.global_variables.len() + id.index(),
            ))
        } else {
            Err(dangling("player variable", id.index()))
        }
    }

    fn subroutine_symbol(&self, id: SubroutineId) -> Result<SymbolId, IrError> {
        if self.program.subroutines.contains(id) {
            Ok(SymbolId::from_index(
                self.program.global_variables.len()
                    + self.program.player_variables.len()
                    + id.index(),
            ))
        } else {
            Err(dangling("subroutine", id.index()))
        }
    }
}

fn dangling(what: &'static str, id: usize) -> IrError {
    IrError::DanglingReference {
        what,
        id: id as u32,
    }
}
