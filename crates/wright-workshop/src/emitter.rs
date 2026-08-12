//! Deterministic localized Workshop emitter.
//!
//! Serializes validated Workshop IR into localized Workshop text with a
//! selectable output locale. Canonical catalog identities resolve to
//! locale-specific spellings; unknown or unsupported identities produce
//! structured diagnostics instead of partial silent output. The formatting
//! is fixed and presentation-canonical, so the same WIR/config emits
//! byte-stable text that reparses to equivalent WIR.

use std::fmt::Write;

use wright_ir::wir;

use crate::catalog::{Catalog, Kind, Locale};
use crate::error::{Result, WorkshopError};

/// Emit a Workshop IR program as localized Workshop text.
pub fn emit(program: &wir::Program, catalog: &Catalog, locale: &Locale) -> Result<String> {
    Emitter {
        program,
        catalog,
        locale: locale.clone(),
        out: String::new(),
    }
    .run()
}

struct Emitter<'a> {
    program: &'a wir::Program,
    catalog: &'a Catalog,
    locale: Locale,
    out: String,
}

impl<'a> Emitter<'a> {
    fn run(mut self) -> Result<String> {
        if !self.program.global_variables.is_empty() || !self.program.player_variables.is_empty() {
            self.line(0, "variables {")?;
            if !self.program.global_variables.is_empty() {
                self.line(1, "global:")?;
                for variable in self.program.global_variables.iter() {
                    self.line(2, &format!("{}: {}", variable.index, variable.name))?;
                }
            }
            if !self.program.player_variables.is_empty() {
                self.line(1, "player:")?;
                for variable in self.program.player_variables.iter() {
                    self.line(2, &format!("{}: {}", variable.index, variable.name))?;
                }
            }
            self.line(0, "}")?;
            self.out.push('\n');
        }
        if !self.program.subroutines.is_empty() {
            self.line(0, "subroutines {")?;
            for subroutine in self.program.subroutines.iter() {
                self.line(1, &format!("{}: {}", subroutine.index, subroutine.name))?;
            }
            self.line(0, "}")?;
            self.out.push('\n');
        }
        for (index, rule) in self.program.rules.iter().enumerate() {
            if index > 0 {
                self.out.push('\n');
            }
            self.rule(rule)?;
        }
        Ok(self.out)
    }

    fn rule(&mut self, rule: &wir::Rule) -> Result<()> {
        self.line(0, &format!("rule (\"{}\") {{", escape_string(&rule.name)))?;
        self.line(1, "event {")?;
        match &rule.event {
            wir::Event::Global => {
                self.line(2, "Ongoing - Global;")?;
            }
            wir::Event::EachPlayer => {
                self.line(2, "Ongoing - Each Player;")?;
                self.line(2, "All;")?;
                self.line(2, "All;")?;
            }
            wir::Event::Subroutine(subroutine) => {
                self.line(2, "Subroutine;")?;
                let name = self
                    .program
                    .subroutines
                    .get(*subroutine)
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| "<dangling>".to_string());
                self.line(2, &format!("{name};"))?;
            }
        }
        self.line(1, "}")?;
        if !rule.conditions.is_empty() {
            self.line(1, "conditions {")?;
            for condition in &rule.conditions {
                let mut text = String::new();
                self.value(*condition, &mut text)?;
                self.line(2, &format!("{text};"))?;
            }
            self.line(1, "}")?;
        }
        if !rule.actions.is_empty() {
            self.line(1, "actions {")?;
            for action in &rule.actions {
                self.action(*action, 2)?;
            }
            self.line(1, "}")?;
        }
        self.line(0, "}")?;
        Ok(())
    }

    fn action(&mut self, id: wir::ActionId, level: usize) -> Result<()> {
        let Some(action) = self.program.actions.get(id) else {
            return Err(WorkshopError::Malformed {
                message: format!("dangling action {id}"),
                span: None,
            });
        };
        match action {
            wir::Action::SetGlobalVariable {
                variable, value, ..
            } => {
                let name = self.global_name(*variable)?;
                let mut value_text = String::new();
                self.value(*value, &mut value_text)?;
                self.line(
                    level,
                    &format!("Set Global Variable({name}, {value_text});"),
                )?;
            }
            wir::Action::ModifyGlobalVariable {
                variable,
                op,
                value,
                ..
            } => {
                let name = self.global_name(*variable)?;
                let op = self.modify_op_spelling(*op)?;
                let mut value_text = String::new();
                self.value(*value, &mut value_text)?;
                self.line(
                    level,
                    &format!("Modify Global Variable({name}, {op}, {value_text});"),
                )?;
            }
            wir::Action::SetPlayerVariable {
                player,
                variable,
                value,
                ..
            } => {
                let mut player_text = String::new();
                self.value(*player, &mut player_text)?;
                let name = self.player_name(*variable)?;
                let mut value_text = String::new();
                self.value(*value, &mut value_text)?;
                self.line(
                    level,
                    &format!("Set Player Variable({player_text}, {name}, {value_text});"),
                )?;
            }
            wir::Action::ModifyPlayerVariable {
                player,
                variable,
                op,
                value,
                ..
            } => {
                let mut player_text = String::new();
                self.value(*player, &mut player_text)?;
                let name = self.player_name(*variable)?;
                let op = self.modify_op_spelling(*op)?;
                let mut value_text = String::new();
                self.value(*value, &mut value_text)?;
                self.line(
                    level,
                    &format!("Modify Player Variable({player_text}, {name}, {op}, {value_text});"),
                )?;
            }
            wir::Action::CallSubroutine { subroutine, .. } => {
                let name = self
                    .program
                    .subroutines
                    .get(*subroutine)
                    .map(|s| s.name.clone())
                    .ok_or_else(|| WorkshopError::Unknown {
                        kind: "subroutine",
                        spelling: format!("<{subroutine}>"),
                        locale: self.locale.clone(),
                        span: None,
                    })?;
                self.line(level, &format!("Call Subroutine({name});"))?;
            }
            wir::Action::If {
                branches,
                else_body,
                ..
            } => {
                for (index, branch) in branches.iter().enumerate() {
                    let mut condition = String::new();
                    self.value(branch.condition, &mut condition)?;
                    let keyword = if index == 0 { "If" } else { "Else If" };
                    self.line(level, &format!("{keyword}({condition});"))?;
                    for action in &branch.body {
                        self.action(*action, level + 1)?;
                    }
                }
                if let Some(else_body) = else_body {
                    self.line(level, "Else;")?;
                    for action in else_body {
                        self.action(*action, level + 1)?;
                    }
                }
                self.line(level, "End;")?;
            }
            wir::Action::While {
                condition, body, ..
            } => {
                let mut text = String::new();
                self.value(*condition, &mut text)?;
                self.line(level, &format!("While({text});"))?;
                for action in body {
                    self.action(*action, level + 1)?;
                }
                self.line(level, "End;")?;
            }
            wir::Action::ForGlobalVariable {
                variable,
                start,
                stop,
                step,
                body,
                ..
            } => {
                let name = self.global_name(*variable)?;
                let mut start_text = String::new();
                let mut stop_text = String::new();
                let mut step_text = String::new();
                self.value(*start, &mut start_text)?;
                self.value(*stop, &mut stop_text)?;
                self.value(*step, &mut step_text)?;
                self.line(
                    level,
                    &format!(
                        "For Global Variable({name}, {start_text}, {stop_text}, {step_text});"
                    ),
                )?;
                for action in body {
                    self.action(*action, level + 1)?;
                }
                self.line(level, "End;")?;
            }
            wir::Action::Debug { .. } | wir::Action::Print { .. } => {
                return Err(WorkshopError::Unsupported {
                    message:
                        "Debug/Print actions have no Workshop spelling; emit the underlying effect"
                            .to_string(),
                    span: None,
                });
            }
            wir::Action::Call { name, args, .. } => {
                let spelling = self
                    .catalog
                    .spelling(Kind::Action, &self.locale, name)
                    .ok_or_else(|| WorkshopError::Unknown {
                        kind: "action",
                        spelling: name.clone(),
                        locale: self.locale.clone(),
                        span: None,
                    })?;
                if args.is_empty() {
                    self.line(level, &format!("{spelling};"))?;
                } else {
                    let mut args_text = String::new();
                    self.args(args, &mut args_text)?;
                    self.line(level, &format!("{spelling}({args_text});"))?;
                }
            }
        }
        Ok(())
    }

    fn args(&mut self, args: &[wir::ValueId], out: &mut String) -> Result<()> {
        for (index, arg) in args.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            self.value(*arg, out)?;
        }
        Ok(())
    }

    fn value(&mut self, id: wir::ValueId, out: &mut String) -> Result<()> {
        let Some(node) = self.program.values.get(id) else {
            return Err(WorkshopError::Malformed {
                message: format!("dangling value {id}"),
                span: None,
            });
        };
        match &node.value {
            wir::Value::Number(value) => write!(out, "{value}").unwrap(),
            wir::Value::String(value) => write!(out, "\"{}\"", escape_string(value)).unwrap(),
            wir::Value::Bool(true) => out.push_str("True"),
            wir::Value::Bool(false) => out.push_str("False"),
            wir::Value::Null => out.push_str("Null"),
            wir::Value::Array(elements) => {
                out.push_str("Array(");
                self.args(elements, out)?;
                out.push(')');
            }
            wir::Value::Vector { x, y, z } => {
                out.push_str("Vector(");
                self.value(*x, out)?;
                out.push_str(", ");
                self.value(*y, out)?;
                out.push_str(", ");
                self.value(*z, out)?;
                out.push(')');
            }
            wir::Value::Enum { value_type, value } => {
                let spelling = self
                    .catalog
                    .enum_spelling(value_type, &self.locale, value)
                    .ok_or_else(|| WorkshopError::Unknown {
                        kind: "enum member",
                        spelling: format!("{value_type}.{value}"),
                        locale: self.locale.clone(),
                        span: None,
                    })?;
                // Color values use the constructor form; other domains use
                // bare member spellings (the canonical corpus form).
                if value_type == "Color" {
                    write!(out, "Color({spelling})").unwrap();
                } else {
                    out.push_str(spelling);
                }
            }
            wir::Value::GlobalVariable(variable) => {
                let name = self.global_name(*variable)?;
                write!(out, "Global.{name}").unwrap();
            }
            wir::Value::PlayerVariable { player, variable } => {
                self.value(*player, out)?;
                let name = self.player_name(*variable)?;
                write!(out, ".{name}").unwrap();
            }
            wir::Value::EventPlayer => out.push_str("Event Player"),
            wir::Value::Call { name, args } => {
                if is_comparison_operator(name) {
                    // Canonical form: Compare(a, op, b).
                    if args.len() != 2 {
                        return Err(WorkshopError::Malformed {
                            message: format!("comparison call '{name}' must have 2 args"),
                            span: None,
                        });
                    }
                    out.push_str("Compare(");
                    self.value(args[0], out)?;
                    write!(out, ", {name}, ").unwrap();
                    self.value(args[1], out)?;
                    out.push(')');
                    return Ok(());
                }
                let spelling = self
                    .catalog
                    .spelling(Kind::Value, &self.locale, name)
                    .ok_or_else(|| WorkshopError::Unknown {
                        kind: "value",
                        spelling: name.clone(),
                        locale: self.locale.clone(),
                        span: None,
                    })?;
                if args.is_empty() {
                    // Constants (e.g. Empty Array) emit as bare spellings.
                    out.push_str(spelling);
                } else {
                    out.push_str(spelling);
                    out.push('(');
                    self.args(args, out)?;
                    out.push(')');
                }
            }
        }
        Ok(())
    }

    fn modify_op_spelling(&self, op: wir::ModifyOp) -> Result<&'static str> {
        Ok(match op {
            wir::ModifyOp::Add => "Add",
            wir::ModifyOp::Subtract => "Subtract",
            wir::ModifyOp::Multiply => "Multiply",
            wir::ModifyOp::Divide => "Divide",
            wir::ModifyOp::Modulo => "Modulo",
            wir::ModifyOp::RaiseToPower => "Raise To Power",
            wir::ModifyOp::AppendToArray => "Append To Array",
            wir::ModifyOp::RemoveFromArray => "Remove From Array",
        })
    }

    fn global_name(&self, id: wir::GlobalVarId) -> Result<String> {
        self.program
            .global_variables
            .get(id)
            .map(|variable| variable.name.clone())
            .ok_or_else(|| WorkshopError::Unknown {
                kind: "global variable",
                spelling: format!("<{id}>"),
                locale: self.locale.clone(),
                span: None,
            })
    }

    fn player_name(&self, id: wir::PlayerVarId) -> Result<String> {
        self.program
            .player_variables
            .get(id)
            .map(|variable| variable.name.clone())
            .ok_or_else(|| WorkshopError::Unknown {
                kind: "player variable",
                spelling: format!("<{id}>"),
                locale: self.locale.clone(),
                span: None,
            })
    }

    fn line(&mut self, level: usize, text: &str) -> Result<()> {
        for _ in 0..level {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
        Ok(())
    }
}

fn is_comparison_operator(name: &str) -> bool {
    matches!(name, "==" | "!=" | "<" | "<=" | ">" | ">=")
}

fn escape_string(value: &str) -> String {
    value.replace('"', "\\\"")
}
