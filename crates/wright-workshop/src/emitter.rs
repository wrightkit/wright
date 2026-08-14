//! Deterministic localized Workshop emitter.
//!
//! Serializes validated Workshop IR into localized Workshop text with a
//! selectable output locale. Canonical catalog identities resolve to
//! locale-specific spellings; unknown or unsupported identities produce
//! structured diagnostics instead of partial silent output. The formatting
//! is fixed and presentation-canonical, so the same WIR/config emits
//! byte-stable text that reparses to equivalent WIR — except for the
//! `settings` section: settings-bearing emissions are deliberately rejected
//! by the Workshop parser (a `.ws` decompiler is a non-goal, #86).

use std::fmt::Write;

use wright_ir::settings::table::{self, KeyKind, PathPart};
use wright_ir::settings::{Settings as SettingsTree, SettingsNode};
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

impl Emitter<'_> {
    fn run(mut self) -> Result<String> {
        // Section order: settings, variables, subroutines, rules.
        if let Some(settings) = &self.program.settings {
            self.emit_settings(settings)?;
            self.out.push('\n');
        }
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
        // Rules with no actions are dropped, matching the pinned oracle
        // (pass-only and condition-without-actions rules emit nothing, #87).
        let mut emitted_rules = 0;
        for rule in self.program.rules.iter() {
            if rule.actions.is_empty() {
                continue;
            }
            if emitted_rules > 0 {
                self.out.push('\n');
            }
            self.rule(rule)?;
            emitted_rules += 1;
        }
        // The oracle's raw artifact ends with a trailing blank line (the
        // committed snapshots strip it via the acquisition normalizer; the
        // pinned oracle's own output keeps it, #87).
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
        Ok(self.out)
    }

    /// Emit the `settings { ... }` section from the validated settings
    /// carrier, table-driven (fixture-evidenced names, #86). Only runs on
    /// validated programs, so unknown keys cannot reach this point.
    fn emit_settings(&mut self, settings: &SettingsTree) -> Result<()> {
        self.line(0, "settings {")?;
        for child in &settings.children {
            let SettingsNode::Group { name, children, .. } = child else {
                return Err(self.malformed("settings block children must be groups"));
            };
            match name.as_str() {
                "main" | "lobby" => {
                    self.line(1, &format!("{name} {{"))?;
                    for member in children {
                        self.settings_member(member, 2, &[PathPart::Part(name)])?;
                    }
                    self.line(1, "}")?;
                }
                "gamemodes" => self.emit_modes(children)?,
                "heroes" => self.emit_heroes(children)?,
                other => {
                    return Err(
                        self.malformed(format!("unknown top-level settings group '{other}'"))
                    );
                }
            }
        }
        self.line(0, "}")?;
        Ok(())
    }

    /// Emit the `modes { <Mode> { ... } }` block of a gamemodes group.
    fn emit_modes(&mut self, modes: &[SettingsNode]) -> Result<()> {
        self.line(1, "modes {")?;
        for mode in modes {
            let SettingsNode::Group { name, children, .. } = mode else {
                return Err(self.malformed("mode entries must be groups"));
            };
            let display = table::mode_name(name)
                .ok_or_else(|| self.malformed(format!("unknown game mode '{name}'")))?;
            // `enabled: false` prefixes the mode header; true renders with no
            // prefix (only false is evidenced in the corpus, #86).
            let disabled = children.iter().any(|member| {
                matches!(
                    member,
                    SettingsNode::Bool { name: n, value: false, .. } if n == "enabled"
                )
            });
            let header = if disabled {
                format!("disabled {display}")
            } else {
                display.to_string()
            };
            self.line(2, &format!("{header} {{"))?;
            for member in children {
                if matches!(member, SettingsNode::Bool { name: n, .. } if n == "enabled") {
                    continue;
                }
                self.settings_member(
                    member,
                    3,
                    &[PathPart::Part("gamemodes"), PathPart::Part(name)],
                )?;
            }
            self.line(2, "}")?;
        }
        self.line(1, "}")?;
        Ok(())
    }

    /// Emit the `heroes { <Team> { ... } }` block of a heroes group.
    fn emit_heroes(&mut self, teams: &[SettingsNode]) -> Result<()> {
        self.line(1, "heroes {")?;
        for team in teams {
            let SettingsNode::Group { name, children, .. } = team else {
                return Err(self.malformed("team entries must be groups"));
            };
            let display = table::team_name(name)
                .ok_or_else(|| self.malformed(format!("unknown team '{name}'")))?;
            self.line(2, &format!("{display} {{"))?;
            for member in children {
                match member {
                    SettingsNode::Group { name, children, .. } => {
                        let hero = table::hero_name(name)
                            .ok_or_else(|| self.malformed(format!("unknown hero '{name}'")))?;
                        self.line(3, &format!("{hero} {{"))?;
                        for inner in children {
                            self.settings_member(
                                inner,
                                4,
                                &[PathPart::Part("heroes"), PathPart::Team, PathPart::Hero],
                            )?;
                        }
                        self.line(3, "}")?;
                    }
                    other => {
                        self.settings_member(other, 3, &[PathPart::Part("heroes"), PathPart::Team])?
                    }
                }
            }
            self.line(2, "}")?;
        }
        self.line(1, "}")?;
        Ok(())
    }

    /// Emit one leaf-level settings member (`Name: value`, lists as blocks).
    fn settings_member(
        &mut self,
        node: &SettingsNode,
        level: usize,
        path: &[PathPart],
    ) -> Result<()> {
        let name = node.name();
        let mut full = path.to_vec();
        full.push(PathPart::Part(name));
        let entry = table::lookup(&full).ok_or_else(|| {
            self.malformed(format!(
                "settings key '{}' is outside the emission table",
                table::path_string(&full)
            ))
        })?;
        match (node, &entry.kind) {
            (SettingsNode::String { value, .. }, KeyKind::String) => {
                self.line(
                    level,
                    &format!(
                        "{}: \"{}\"",
                        entry.workshop_name,
                        escape_settings_string(value)
                    ),
                )?;
            }
            (SettingsNode::String { value, .. }, KeyKind::Enum(domain)) => {
                let display = table::enum_name(domain, value).ok_or_else(|| {
                    self.malformed(format!("unknown value '{value}' for settings key '{name}'"))
                })?;
                self.line(level, &format!("{}: {display}", entry.workshop_name))?;
            }
            (SettingsNode::Number { value, .. }, KeyKind::Number) => {
                self.line(
                    level,
                    &format!("{}: {}", entry.workshop_name, format_number(*value)),
                )?;
            }
            (SettingsNode::Number { value, .. }, KeyKind::Percent) => {
                self.line(
                    level,
                    &format!("{}: {}%", entry.workshop_name, format_number(*value)),
                )?;
            }
            (SettingsNode::Bool { value, .. }, KeyKind::Bool) => {
                let rendered = if *value { "On" } else { "Off" };
                self.line(level, &format!("{}: {rendered}", entry.workshop_name))?;
            }
            (SettingsNode::List { elements, .. }, KeyKind::ListMap) => {
                self.line(level, &format!("{} {{", entry.workshop_name))?;
                for element in elements {
                    let display = table::map_name(&element.value).ok_or_else(|| {
                        self.malformed(format!(
                            "unknown map '{}' in settings list '{name}'",
                            element.value
                        ))
                    })?;
                    self.line(level + 1, display)?;
                }
                self.line(level, "}")?;
            }
            (SettingsNode::List { elements, .. }, KeyKind::ListHero) => {
                self.line(level, &format!("{} {{", entry.workshop_name))?;
                for element in elements {
                    let display = table::hero_name(&element.value).ok_or_else(|| {
                        self.malformed(format!(
                            "unknown hero '{}' in settings list '{name}'",
                            element.value
                        ))
                    })?;
                    self.line(level + 1, display)?;
                }
                self.line(level, "}")?;
            }
            _ => {
                return Err(self.malformed(format!(
                    "settings key '{name}' does not match its table kind"
                )));
            }
        }
        Ok(())
    }

    fn malformed(&self, message: impl Into<String>) -> WorkshopError {
        WorkshopError::Malformed {
            message: message.into(),
            span: None,
        }
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
                // Reference normalization: comparison conditions render
                // infix; other conditions render as `value == True`.
                if let Some(wir::Value::Call { name, args }) =
                    self.program.values.get(*condition).map(|node| &node.value)
                {
                    if is_comparison_operator(name) && args.len() == 2 {
                        self.value(args[0], &mut text)?;
                        write!(text, " {name} ").unwrap();
                        self.value(args[1], &mut text)?;
                    } else {
                        self.value(*condition, &mut text)?;
                        text.push_str(" == True");
                    }
                } else {
                    self.value(*condition, &mut text)?;
                    text.push_str(" == True");
                }
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
            wir::Action::Debug { value, .. } => {
                // `debug(value)` displays the value as HUD text. The
                // reference formats values with type-aware machinery; Wright
                // emits a semantically equivalent but presentation-simpler
                // Create HUD Text (documented intentional difference).
                self.emit_hud_text(*value, level, true)?;
            }
            wir::Action::Print { message, .. } => {
                self.emit_hud_text(*message, level, false)?;
            }
            wir::Action::Call { name, args, .. } => {
                // Native `.opy` action names map to canonical catalog ids at
                // emission (presentation concern).
                let canonical = match name.as_str() {
                    "createBeam" => Some("createBeamEffect"),
                    _ => None,
                };
                let spelling = if let Some(canonical) = canonical {
                    self.catalog
                        .spelling(Kind::Action, &self.locale, canonical)
                        .ok_or_else(|| WorkshopError::Unknown {
                            kind: "action",
                            spelling: canonical.to_string(),
                            locale: self.locale.clone(),
                            span: None,
                        })?
                        .to_string()
                } else {
                    self.catalog
                        .spelling(Kind::Action, &self.locale, name)
                        .ok_or_else(|| WorkshopError::Unknown {
                            kind: "action",
                            spelling: name.clone(),
                            locale: self.locale.clone(),
                            span: None,
                        })?
                        .to_string()
                };
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

    /// Emit a `debug`/`print` action as a `Create HUD Text` effect.
    ///
    /// `debug` renders the value into the HUD body; `print` renders the
    /// message directly (a `format` value already carries the text).
    fn emit_hud_text(&mut self, value: wir::ValueId, level: usize, is_debug: bool) -> Result<()> {
        let mut body = String::new();
        if is_debug {
            // Display the value in the HUD body: Custom String("{0}", value).
            body.push_str("Custom String(\"{0}\", ");
            self.value(value, &mut body)?;
            body.push(')');
        } else {
            self.value(value, &mut body)?;
        }
        // Create HUD Text(All Players(All Teams), header, body, icon, ...).
        self.line(
            level,
            &format!(
                "Create HUD Text(All Players(All Teams), Null, {body}, Null, Null, Left, -9999, Color(White), Null, Null, Visible To and String, Null, Null);"
            ),
        )?;
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
            wir::Value::Number(value) => {
                out.push_str(&format_number(*value));
            }
            wir::Value::String(value) => {
                // Value-position strings wrap in `Custom String("...")` with
                // re-escaped content and long-string splitting, the pinned
                // oracle's spelling (evidence: array elements, initializers,
                // assignments, call arguments, comparisons — #87). The only
                // bare string value is the `Custom String` text argument,
                // handled in the call arm below.
                self.emit_string_value(value, out)?;
            }
            wir::Value::Bool(true) => out.push_str("True"),
            wir::Value::Bool(false) => out.push_str("False"),
            wir::Value::Null => out.push_str("Null"),
            wir::Value::Array(elements) => {
                if elements.is_empty() {
                    // The canonical empty-array constant (reference emission).
                    out.push_str("Empty Array");
                } else {
                    out.push_str("Array(");
                    self.args(elements, out)?;
                    out.push(')');
                }
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
                // The `.opy`-layer `EffectReeval` domain is the same Workshop
                // reevaluation domain as `HudReeval` (member ids align); map
                // it at emission to avoid catalog domain collisions.
                let catalog_domain = if value_type == "EffectReeval" {
                    "HudReeval"
                } else {
                    value_type
                };
                let spelling = self
                    .catalog
                    .enum_spelling(catalog_domain, &self.locale, value)
                    .ok_or_else(|| WorkshopError::Unknown {
                        kind: "enum member",
                        spelling: format!("{catalog_domain}.{value}"),
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
                // Unary minus renders as Multiply(-1, x); the reference folds
                // literal negation, handled by the compat constant-fold pass.
                if name == "-" && args.len() == 1 {
                    out.push_str("Multiply(-1, ");
                    self.value(args[0], out)?;
                    out.push(')');
                    return Ok(());
                }
                // `getAllPlayers()` is OverPy's All Players(All Teams).
                if name == "getAllPlayers" && args.is_empty() {
                    out.push_str("All Players(All Teams)");
                    return Ok(());
                }
                // Binary arithmetic operators and native `.opy` source names
                // map to canonical catalog ids at emission (presentation
                // concern; the compat pass folds constants to match the
                // reference exactly).
                let canonical = match name.as_str() {
                    "+" => Some("add"),
                    "-" => Some("subtract"),
                    "*" => Some("multiply"),
                    "/" => Some("divide"),
                    "len" => Some("countOf"),
                    "abs" => Some("absoluteValue"),
                    "sqrt" => Some("squareRoot"),
                    "createBeam" => Some("createBeamEffect"),
                    "random.uniform" => Some("randomReal"),
                    "random.choice" => Some("randomValueInArray"),
                    "format" => Some("customString"),
                    _ => None,
                };
                let spelling = if let Some(canonical) = canonical {
                    self.catalog
                        .spelling(Kind::Value, &self.locale, canonical)
                        .ok_or_else(|| WorkshopError::Unknown {
                            kind: "value",
                            spelling: canonical.to_string(),
                            locale: self.locale.clone(),
                            span: None,
                        })?
                        .to_string()
                } else {
                    self.catalog
                        .spelling(Kind::Value, &self.locale, name)
                        .ok_or_else(|| WorkshopError::Unknown {
                            kind: "value",
                            spelling: name.clone(),
                            locale: self.locale.clone(),
                            span: None,
                        })?
                        .to_string()
                };
                // `format` (frontend) and `customString` (parsed ws text) are
                // the same node.
                let is_custom_string = canonical == Some("customString") || name == "customString";
                if args.is_empty() {
                    // Constants (e.g. Empty Array) emit as bare spellings.
                    out.push_str(&spelling);
                } else if is_custom_string {
                    // The `Custom String` text argument stays bare (the
                    // oracle spelling); the remaining arguments are values
                    // and wrap (#87).
                    out.push_str(&spelling);
                    out.push('(');
                    self.bare_string_value(args[0], out)?;
                    if args.len() > 1 {
                        out.push_str(", ");
                    }
                    self.args(&args[1..], out)?;
                    out.push(')');
                } else {
                    out.push_str(&spelling);
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

    /// Render a value that must stay a bare string (the `Custom String` text
    /// argument). Any non-string value falls back to the normal renderer.
    fn bare_string_value(&mut self, id: wir::ValueId, out: &mut String) -> Result<()> {
        let Some(node) = self.program.values.get(id) else {
            return Err(WorkshopError::Malformed {
                message: format!("dangling value {id}"),
                span: None,
            });
        };
        if let wir::Value::String(value) = &node.value {
            write!(out, "\"{}\"", escape_value_string(value)).unwrap();
            return Ok(());
        }
        self.value(id, out)
    }

    /// Emit a value-position string as `Custom String("...")`, splitting it
    /// into a continuation chain when it exceeds the Workshop 128-char limit
    /// (#87).
    fn emit_string_value(&mut self, value: &str, out: &mut String) -> Result<()> {
        let spelling = self
            .catalog
            .spelling(Kind::Value, &self.locale, "customString")
            .ok_or_else(|| WorkshopError::Unknown {
                kind: "value",
                spelling: "customString".to_string(),
                locale: self.locale.clone(),
                span: None,
            })?
            .to_string();
        let segments = split_string(value);
        self.emit_string_chain(&spelling, &segments, out)
    }

    /// Emit the nested continuation chain
    /// `Custom String(seg0, Custom String(seg1, ...))`; segment texts are
    /// pre-escaped, non-final segments carry the `{0}` placeholder.
    fn emit_string_chain(
        &mut self,
        spelling: &str,
        segments: &[String],
        out: &mut String,
    ) -> Result<()> {
        let Some((first, rest)) = segments.split_first() else {
            return Ok(());
        };
        out.push_str(spelling);
        out.push('(');
        write!(out, "\"{first}\"").unwrap();
        if !rest.is_empty() {
            out.push_str(", ");
            self.emit_string_chain(spelling, rest, out)?;
        }
        out.push(')');
        Ok(())
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

/// Format a float like the reference frontend: integers print without a
/// decimal point, and non-integers print the shortest round-trip
/// representation truncated to 16 significant digits (OverPy behavior;
/// evidence: the pinned oracle snapshots).
fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return format!("{value}");
    }
    if value == 0.0 {
        return "0".to_string();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    truncate_significant(&format!("{value}"), 16)
}

/// Keep at most `max_digits` significant digits of a decimal string,
/// truncating (not rounding) and expanding any exponent form.
fn truncate_significant(text: &str, max_digits: usize) -> String {
    let (mantissa, exponent) = match text.find('e').or_else(|| text.find('E')) {
        Some(index) => {
            let exponent: i32 = text[index + 1..].parse().unwrap_or(0);
            (&text[..index], exponent)
        }
        None => (text, 0),
    };
    let (sign, mantissa) = mantissa
        .strip_prefix('-')
        .map_or(("", mantissa), |rest| ("-", rest));
    let digits: Vec<char> = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let before_dot = mantissa.find('.').unwrap_or(mantissa.len());
    let point = before_dot as i32 + exponent;
    let first_nonzero = digits
        .iter()
        .position(|c| *c != '0')
        .unwrap_or(digits.len());
    let mut digits = digits;
    if digits.len() - first_nonzero > max_digits {
        digits.truncate(first_nonzero + max_digits);
    }
    let mut out = String::from(sign);
    if point <= 0 {
        out.push_str("0.");
        for _ in 0..(-point) {
            out.push('0');
        }
        for c in &digits {
            out.push(*c);
        }
    } else if point as usize >= digits.len() {
        for c in &digits {
            out.push(*c);
        }
        for _ in 0..(point as usize - digits.len()) {
            out.push('0');
        }
    } else {
        for (index, c) in digits.iter().enumerate() {
            if index == point as usize {
                out.push('.');
            }
            out.push(*c);
        }
    }
    out
}

fn is_comparison_operator(name: &str) -> bool {
    matches!(name, "==" | "!=" | "<" | "<=" | ">" | ">=")
}

fn escape_string(value: &str) -> String {
    value.replace('"', "\\\"")
}

/// Re-escape a decoded value string the way the pinned oracle does (#87):
/// `\`, `"`, newline, and carriage return re-escape; tabs pass through raw
/// (byte-measured oracle behavior: `a\tb` emits a real tab, `a\nb` emits the
/// literal two-character `\n`).
fn escape_value_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

/// Split a decoded string per the oracle's long-string rule (#87): when the
/// decoded length exceeds the Workshop 128-char limit, non-final segments
/// hold exactly 125 decoded chars and are emitted with a `{0}` continuation
/// placeholder (128 total text chars), chained as nested `Custom String`
/// arguments; the final segment holds the remainder without a placeholder.
/// Segment texts are re-escaped. Byte-measured basis: chunk sizes are
/// counted on the decoded string (70 escaped newlines — 140 escaped chars,
/// 70 decoded — emit unsplit; 129 decoded newlines split at 125 decoded).
fn split_string(value: &str) -> Vec<String> {
    if value.chars().count() <= 128 {
        return vec![escape_value_string(value)];
    }
    let mut segments = Vec::new();
    let mut rest = value;
    while rest.chars().count() > 125 {
        let chunk: String = rest.chars().take(125).collect();
        let mut text = escape_value_string(&chunk);
        text.push_str("{0}");
        segments.push(text);
        rest = &rest[chunk.len()..];
    }
    if !rest.is_empty() {
        segments.push(escape_value_string(rest));
    }
    segments
}

/// Escape a settings string value the way the pinned oracle does: every
/// decode the JSONC parser performed is re-escaped, so decoded values
/// round-trip to the oracle's spelling. Evidence: the inputhud description
/// (`\n` in the source block) is emitted by the oracle as the literal
/// two-character sequence `\n` in the Workshop settings section.
fn escape_settings_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::format_number;

    #[test]
    fn integers_print_without_decimals() {
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(100.0), "100");
        assert_eq!(format_number(-3.0), "-3");
    }

    #[test]
    fn floats_match_reference_precision() {
        assert_eq!(format_number(1.8106601717798212), "1.810660171779821");
        assert_eq!(format_number(-1.2803300858899105), "-1.280330085889910");
        assert_eq!(format_number(0.016), "0.016");
        assert_eq!(format_number(0.125), "0.125");
        assert_eq!(format_number(1.5), "1.5");
    }
}
