use workshop_rs::wir::{self, Action, ActionId, Event, Rule, RuleId, Value, ValueId};

/// Read-only canonical semantic facts exposed to Wright-owned policies.
pub struct SemanticFacts<'a> {
    program: &'a wir::Program,
}

impl<'a> SemanticFacts<'a> {
    pub fn new(program: &'a wir::Program) -> Self {
        Self { program }
    }

    pub fn rule(&self, id: RuleId) -> Option<RuleFacts<'a>> {
        self.program.rules.get(id).map(|rule| RuleFacts {
            program: self.program,
            id,
            rule,
        })
    }
}

pub struct RuleFacts<'a> {
    program: &'a wir::Program,
    id: RuleId,
    rule: &'a Rule,
}

impl<'a> RuleFacts<'a> {
    pub fn id(&self) -> RuleId {
        self.id
    }

    pub fn event(&self) -> &Event {
        &self.rule.event
    }

    pub fn conditions(&self) -> &[ValueId] {
        &self.rule.conditions
    }

    pub fn actions(&self) -> &[ActionId] {
        &self.rule.actions
    }

    pub fn action(&self, id: ActionId) -> Option<&'a Action> {
        self.program.actions.get(id)
    }

    pub fn value(&self, id: ValueId) -> Option<&'a Value> {
        self.program.values.get(id).map(|node| &node.value)
    }

    pub fn program(&self) -> &'a wir::Program {
        self.program
    }
}
