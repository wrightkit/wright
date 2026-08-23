//! Workshop IR → OPY reconstruction (issue #124).
//!
//! Consumes a validated [`workshop_rs::wir::Program`] and emits deterministic,
//! byte-stable canonical OPY source that the native [`crate::compile`]
//! frontend accepts and that re-lowers to a structurally equivalent WIR
//! program under `workshop_rs::roundtrip::equivalent`.
//!
//! Scope and ownership:
//!
//! * Builtin action/value/member/enum identities resolve only through the
//!   OPY semantic compatibility manifest ([`Manifest`]) and the Workshop
//!   catalog ([`Catalog`]) — no new content/signature tables and no invented
//!   OPY syntax.
//! * Reconstructed OPY is simple low-level valid OPY: it does not recover
//!   comments, macros, functions, or source abstractions. Names must be valid
//!   OPY identifiers; calls must use the OPY source names the manifest
//!   declares (`len`, `wait`, `playEffect`, …), because the frontend's own
//!   lowering stamps those names into the recompiled WIR and the equivalence
//!   contract compares them exactly.
//! * Every WIR construct the frontend cannot recompile identically is
//!   rejected with a structured [`ReconstructIssue`] naming the construct —
//!   never partial or misleading OPY. This includes the per-player loop
//!   form, disabled rules, arbitrary-player variable targets, negative and
//!   non-finite number literals (the OPY lexer has no negative-literal
//!   token), Workshop-spelled call names with no manifest source form
//!   (`add`, `countOf`, `createBeamEffect`, …), enums outside the manifest's
//!   declared domains, `Remove From Array` modifies, calls the frontend
//!   lowers to dedicated nodes (`debug`, `print`, `append`, `vect`), and any
//!   rule layout the frontend's deterministic re-lowering cannot reproduce
//!   (non-leading initializer rules, out-of-table-order subroutine rules,
//!   unsorted global slots, non-canonical subroutine indices).
//! * `debug`/`print` actions, arrays, vectors, and `format` are emitted in
//!   their OPY source forms (`debug(x)`, `print(x)`, `[...]`, `vect(x, y, z)`,
//!   `"text".format(...)`) from the dedicated WIR nodes; they are reachable
//!   from OPY-derived WIR and are covered by direct unit tests (no Workshop
//!   text spells them).
//!
//! Pipeline: [`reconstruct`] validates the table layout, then emits the
//! declarations (variables, subroutines), the `def` bodies, and the rules in
//! deterministic arena order. Any issue collected anywhere fails the whole
//! reconstruction with all collected diagnostics.

use std::fmt;

use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::source::Span;
use workshop_rs::wir::{self, Action, Event, ModifyOp, Value};

use crate::manifest::{Function, FunctionKind, Manifest};

/// A structured reconstruction diagnostic naming one non-representable WIR
/// construct. Stable `code`, a human-readable `message`, and the offending
/// source span when the WIR carries one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconstructIssue {
    pub code: &'static str,
    pub message: String,
    pub span: Option<Span>,
}

/// All reconstruction failures for one program, in deterministic arena
/// order. The emitter never returns partial output: a non-empty issue list
/// means no OPY was produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconstructError {
    pub issues: Vec<ReconstructIssue>,
}

impl fmt::Display for ReconstructError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, issue) in self.issues.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            let location = match issue.span {
                Some(span) => format!(" at {}:{}", span.start.line, span.start.col),
                None => String::new(),
            };
            write!(f, "{}: {}{location}", issue.code, issue.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for ReconstructError {}

/// Reconstruct a validated WIR program into deterministic OPY source.
///
/// Resolves builtin identities through the built-in OPY semantic manifest
/// and the built-in Workshop catalog (`en-US`), the declared surface for
/// reconstruction (issue #124). Returns an error carrying every
/// non-representable construct diagnostic when the program cannot be
/// reconstructed.
pub fn reconstruct(program: &wir::Program) -> Result<String, ReconstructError> {
    let manifest = match Manifest::builtin() {
        Ok(manifest) => manifest,
        Err(error) => {
            return Err(ReconstructError {
                issues: vec![ReconstructIssue {
                    code: "manifest-error",
                    message: format!(
                        "cannot load the OPY semantic compatibility manifest: {error}"
                    ),
                    span: None,
                }],
            });
        }
    };
    let catalog = match Catalog::builtin() {
        Ok(catalog) => catalog,
        Err(error) => {
            return Err(ReconstructError {
                issues: vec![ReconstructIssue {
                    code: "catalog-error",
                    message: format!("cannot load the Workshop catalog: {error}"),
                    span: None,
                }],
            });
        }
    };
    reconstruct_with(program, manifest, &catalog, &Locale::new("en-US"))
}

/// The context-sensitive form of [`reconstruct`]: resolves identities through
/// the supplied manifest and catalog. The locale selects the catalog
/// spellings used for cross-checks (reconstruction emits OPY, which is
/// locale-independent; `en-US` is the catalog's declared surface).
pub fn reconstruct_with(
    program: &wir::Program,
    manifest: &Manifest,
    catalog: &Catalog,
    locale: &Locale,
) -> Result<String, ReconstructError> {
    let mut emitter = Emitter::new(program, manifest, catalog, locale);
    emitter.run();
    if emitter.issues.is_empty() {
        Ok(emitter.out)
    } else {
        Err(ReconstructError {
            issues: emitter.issues,
        })
    }
}

/// OPY names the parser treats as keywords or literals; a WIR table name that
/// collides with one of these can never be referenced or declared faithfully.
const RESERVED_NAMES: &[&str] = &[
    "true",
    "false",
    "None",
    "null",
    "eventPlayer",
    "rule",
    "def",
    "globalvar",
    "playervar",
    "subroutine",
    "enum",
    "macro",
    "if",
    "for",
    "while",
    "pass",
    "elif",
    "else",
    "in",
    "and",
    "or",
    "not",
];

/// Whether `name` is a valid OPY identifier (the lexer's identifier rule).
fn is_opy_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Binary operator spellings the OPY frontend lowers to `Value::Call`s with
/// the same name (source operators, not Workshop spellings like `add`).
const BINARY_OPS: &[&str] = &[
    "+", "-", "*", "/", "%", "**", "==", "!=", "<", "<=", ">", ">=", "and", "or",
];

/// Call names the frontend lowers to dedicated WIR nodes (never `Call`s).
const DEDICATED_ACTION_NAMES: &[&str] = &["debug", "print", "append"];
const DEDICATED_VALUE_NAMES: &[&str] = &["vect", "range", "chase"];

struct Emitter<'a> {
    program: &'a wir::Program,
    manifest: &'a Manifest,
    catalog: &'a Catalog,
    locale: &'a Locale,
    issues: Vec<ReconstructIssue>,
    out: String,
    /// Subroutine names, for call-vs-subroutine ambiguity checks.
    subroutine_names: std::collections::HashSet<String>,
}

/// The canonical rule layout the frontend's re-lowering reproduces.
struct RuleLayout<'a> {
    /// The leading "Initialize global variables" rule, converted to
    /// declaration initializers.
    global_init: Option<Vec<wir::ActionId>>,
    /// The leading "Initialize player variables" rule, converted to
    /// declaration initializers.
    player_init: Option<Vec<wir::ActionId>>,
    /// Subroutine-body rules (defs), in subroutine table order.
    sub_rules: Vec<&'a wir::Rule>,
    /// Everything else, in input order.
    normal_rules: Vec<&'a wir::Rule>,
}

impl<'a> Emitter<'a> {
    fn new(
        program: &'a wir::Program,
        manifest: &'a Manifest,
        catalog: &'a Catalog,
        locale: &'a Locale,
    ) -> Self {
        let subroutine_names = program
            .subroutines
            .iter()
            .map(|subroutine| subroutine.name.clone())
            .collect();
        Emitter {
            program,
            manifest,
            catalog,
            locale,
            issues: Vec::new(),
            out: String::new(),
            subroutine_names,
        }
    }

    fn run(&mut self) {
        self.validate_tables();
        if self.issues.is_empty() {
            let layout = self.classify_rules();
            if self.issues.is_empty() {
                self.emit_program(&layout);
            }
        }
    }
    // ---- diagnostics ----

    fn issue(&mut self, code: &'static str, message: impl Into<String>, span: Option<Span>) {
        self.issues.push(ReconstructIssue {
            code,
            message: message.into(),
            span,
        });
    }

    // ---- table validation ----

    fn validate_tables(&mut self) {
        if self.program.settings.is_some() {
            self.issue(
                "unsupported-settings",
                "custom-game-settings are outside the reconstruction surface",
                None,
            );
        }
        // Global table: unique names, valid OPY identifiers, non-decreasing
        // slot order (the frontend's re-lowering sorts the table by index,
        // so only slot-ordered input reproduces the same table).
        let mut previous_index: Option<u32> = None;
        for (position, variable) in self.program.global_variables.iter().enumerate() {
            self.check_variable_name(variable.name.as_str(), variable.span, "global variable");
            self.check_duplicate_name(
                variable.name.as_str(),
                position,
                "global variable",
                variable.span,
            );
            if let Some(previous) = previous_index {
                if variable.index < previous {
                    self.issue(
                        "unsupported-global-order",
                        format!(
                            "global variables must be in ascending index order \
                             (slot {} precedes slot {})",
                            previous, variable.index
                        ),
                        variable.span,
                    );
                }
            }
            previous_index = Some(variable.index);
        }
        // Player table: unique names and valid identifiers; player slots are
        // explicit in the `playervar name <index>` form, so no order rule.
        for (position, variable) in self.program.player_variables.iter().enumerate() {
            self.check_variable_name(variable.name.as_str(), variable.span, "player variable");
            self.check_duplicate_name(
                variable.name.as_str(),
                position,
                "player variable",
                variable.span,
            );
        }
        // Subroutine table: unique names, valid identifiers, and indices
        // exactly equal to table position (the OPY `subroutine name`
        // declaration cannot carry an index; the re-lowered index is the
        // table position).
        for (position, subroutine) in self.program.subroutines.iter().enumerate() {
            self.check_variable_name(subroutine.name.as_str(), subroutine.span, "subroutine");
            self.check_duplicate_name(
                subroutine.name.as_str(),
                position,
                "subroutine",
                subroutine.span,
            );
            if subroutine.index as usize != position {
                self.issue(
                    "unsupported-subroutine-index",
                    format!(
                        "subroutine '{}' has index {} but the OPY surface requires \
                         table position {} (subroutine declarations cannot carry an index)",
                        subroutine.name, subroutine.index, position
                    ),
                    subroutine.span,
                );
            }
        }
    }

    fn check_variable_name(&mut self, name: &str, span: Option<Span>, kind: &str) {
        if !is_opy_identifier(name) {
            self.issue(
                "unsupported-name",
                format!(
                    "{kind} name '{name}' is not a valid OPY identifier on the \
                     reconstruction surface"
                ),
                span,
            );
        } else if RESERVED_NAMES.contains(&name) {
            self.issue(
                "unsupported-name",
                format!(
                    "{kind} name '{name}' collides with an OPY keyword or literal \
                     and cannot be referenced on the reconstruction surface"
                ),
                span,
            );
        }
    }

    /// Whether a name repeats an earlier entry of its table (duplicates
    /// cannot be declared or referenced faithfully on the OPY surface).
    fn check_duplicate_name(
        &mut self,
        name: &str,
        position: usize,
        kind: &str,
        span: Option<Span>,
    ) {
        let duplicate = match kind {
            "global variable" => self
                .program
                .global_variables
                .iter()
                .enumerate()
                .take(position)
                .any(|(_, other)| other.name == name),
            "player variable" => self
                .program
                .player_variables
                .iter()
                .enumerate()
                .take(position)
                .any(|(_, other)| other.name == name),
            _ => self
                .program
                .subroutines
                .iter()
                .enumerate()
                .take(position)
                .any(|(_, other)| other.name == name),
        };
        if duplicate {
            self.issue(
                "unsupported-duplicate-name",
                format!("duplicate {kind} name '{name}'"),
                span,
            );
        }
    }

    /// The canonical rule layout: optional leading initializer rules, then
    /// all subroutine-body rules in subroutine table order, then the normal
    /// rules. Any other arrangement cannot be reproduced by the frontend's
    /// deterministic re-lowering and is rejected.
    fn classify_rules(&mut self) -> RuleLayout<'a> {
        let rules: Vec<&wir::Rule> = self.program.rules.iter().collect();
        let mut index = 0;
        let mut global_init = None;
        let mut player_init = None;
        if let Some(rule) = rules.first() {
            if rule.name == "Initialize global variables" {
                global_init = self.canonical_init(rule, true);
                index = 1;
            } else if rule.name == "Initialize player variables" {
                player_init = self.canonical_init(rule, false);
                index = 1;
            }
        }
        if index == 1 {
            if let Some(rule) = rules.get(1) {
                if rule.name == "Initialize player variables" && global_init.is_some() {
                    player_init = self.canonical_init(rule, false);
                    index = 2;
                }
            }
        }

        let mut sub_rules = Vec::new();
        let mut normal_rules = Vec::new();
        let mut in_sub_rules = true;
        for rule in rules.iter().copied().skip(index) {
            match &rule.event {
                Event::Subroutine(_) => {
                    if !in_sub_rules {
                        self.issue(
                            "unsupported-rule-order",
                            format!(
                                "subroutine-body rule '{}' appears after a normal rule; \
                                 the frontend re-lowering emits subroutine rules first",
                                rule.name
                            ),
                            rule.span,
                        );
                    }
                    if !rule.conditions.is_empty() {
                        self.issue(
                            "unsupported-rule-order",
                            format!(
                                "subroutine-body rule '{}' carries conditions; `def` \
                                 bodies cannot express them",
                                rule.name
                            ),
                            rule.span,
                        );
                    }
                    sub_rules.push(rule);
                }
                _ => {
                    in_sub_rules = false;
                    normal_rules.push(rule);
                }
            }
        }

        // Subroutine rules must be in subroutine table order, and each rule
        // must carry the exact name the re-lowering synthesizes for its def.
        let mut expected = 0usize;
        for rule in &sub_rules {
            let Event::Subroutine(subroutine) = &rule.event else {
                continue;
            };
            if subroutine.index() != expected {
                self.issue(
                    "unsupported-rule-order",
                    format!(
                        "subroutine-body rules must appear in subroutine table order; \
                         '{}' is out of order",
                        rule.name
                    ),
                    rule.span,
                );
            }
            expected += 1;
            if let Some(definition) = self.program.subroutines.get(*subroutine) {
                let expected_name = format!("Subroutine {}", definition.name);
                if rule.name != expected_name {
                    self.issue(
                        "unsupported-rule-order",
                        format!(
                            "subroutine-body rule name '{}' does not match the def \
                             form '{}' the frontend synthesizes",
                            rule.name, expected_name
                        ),
                        rule.span,
                    );
                }
            }
        }

        RuleLayout {
            global_init,
            player_init,
            sub_rules,
            normal_rules,
        }
    }

    /// Validate a leading initializer rule: exactly the synthesized shape
    /// (name, event, empty conditions, all-Set actions). Returns the action
    /// ids to convert into declaration initializers, or records an issue.
    fn canonical_init(&mut self, rule: &wir::Rule, global: bool) -> Option<Vec<wir::ActionId>> {
        let expected_name = if global {
            "Initialize global variables"
        } else {
            "Initialize player variables"
        };
        if !rule.conditions.is_empty() {
            self.issue(
                "unsupported-init-rule",
                format!(
                    "initializer rule '{expected_name}' carries conditions; the \
                     frontend synthesizes it from declarations with none"
                ),
                rule.span,
            );
            return None;
        }
        let mut actions = Vec::with_capacity(rule.actions.len());
        for action in &rule.actions {
            let Some(node) = self.program.actions.get(*action) else {
                self.issue("unsupported-dangling", "dangling action id", rule.span);
                return None;
            };
            let set = matches!(
                (global, node),
                (true, Action::SetGlobalVariable { .. })
                    | (false, Action::SetPlayerVariable { .. })
            );
            if !set {
                self.issue(
                    "unsupported-init-rule",
                    format!(
                        "initializer rule '{expected_name}' mixes non-Set actions; \
                         the frontend's synthesized initializer rule is all-Set"
                    ),
                    node.span(),
                );
                return None;
            }
            actions.push(*action);
        }
        Some(actions)
    }

    // ---- emission ----

    fn emit_program(&mut self, layout: &RuleLayout) {
        let global_initializers = self.collect_global_initializers(&layout.global_init);
        let player_initializers = self.collect_player_initializers(&layout.player_init);
        self.check_initializer_slot(&global_initializers);

        // Declarations.
        for (position, variable) in self.program.global_variables.iter().enumerate() {
            self.out.push_str("globalvar ");
            self.out.push_str(&variable.name);
            match global_initializers.get(&position) {
                Some(value) => {
                    self.out.push_str(" = ");
                    self.emit_initializer(*value);
                }
                None => {
                    self.out.push(' ');
                    self.out.push_str(&variable.index.to_string());
                }
            }
            self.out.push('\n');
        }
        for (position, variable) in self.program.player_variables.iter().enumerate() {
            self.out.push_str("playervar ");
            self.out.push_str(&variable.name);
            match player_initializers.get(&position) {
                Some(value) => {
                    self.out.push_str(" = ");
                    self.emit_initializer(*value);
                }
                None => {
                    self.out.push(' ');
                    self.out.push_str(&variable.index.to_string());
                }
            }
            self.out.push('\n');
        }
        if self.program.subroutines.is_empty() {
            self.out.push('\n');
        } else {
            for subroutine in self.program.subroutines.iter() {
                self.out.push_str("subroutine ");
                self.out.push_str(&subroutine.name);
                self.out.push('\n');
            }
            self.out.push('\n');
        }

        // Subroutine bodies.
        for rule in &layout.sub_rules {
            let Event::Subroutine(subroutine) = &rule.event else {
                continue;
            };
            let Some(definition) = self.program.subroutines.get(*subroutine) else {
                continue;
            };
            self.out.push_str("def ");
            self.out.push_str(&definition.name);
            self.out.push_str("():\n");
            self.emit_actions(&rule.actions, 1);
            self.out.push('\n');
        }

        // Rules.
        for rule in &layout.normal_rules {
            if rule.disabled {
                self.issue(
                    "unsupported-disabled-rule",
                    format!(
                        "rule '{}' is disabled; the OPY surface cannot express it",
                        rule.name
                    ),
                    rule.span,
                );
                continue;
            }
            if rule.actions.is_empty() {
                continue;
            }
            self.out.push_str("rule \"");
            self.out.push_str(&rule.name);
            self.out.push_str("\":\n");
            match &rule.event {
                Event::Global => self.out.push_str("    @Event global\n"),
                Event::EachPlayer => self.out.push_str("    @Event eachPlayer\n"),
                Event::EachPlayerWithFilters {
                    team: workshop_rs::wir::EventTeam::All,
                    target: workshop_rs::wir::EventTarget::All,
                } => self.out.push_str("    @Event eachPlayer\n"),
                Event::EachPlayerWithFilters { .. } | Event::Player { .. } => {
                    self.issue(
                        "unsupported-rule-event",
                        format!("rule '{}' uses an event outside the OPY surface", rule.name),
                        rule.span,
                    );
                    continue;
                }
                Event::Subroutine(_) => {
                    self.issue(
                        "unsupported-rule-order",
                        format!(
                            "rule '{}' has a subroutine event outside the def layout",
                            rule.name
                        ),
                        rule.span,
                    );
                    continue;
                }
            }
            for condition in &rule.conditions {
                self.out.push_str("    @Condition ");
                self.emit_value(*condition);
                self.out.push('\n');
            }
            self.emit_actions(&rule.actions, 1);
            self.out.push('\n');
        }
    }

    /// Map initializer rule actions onto declaration positions (table order),
    /// validating the rule's Sets are in table order like the frontend's
    /// synthesized initializer rule.
    fn collect_global_initializers(
        &mut self,
        actions: &Option<Vec<wir::ActionId>>,
    ) -> std::collections::HashMap<usize, wir::ValueId> {
        let mut initializers = std::collections::HashMap::new();
        let Some(actions) = actions else {
            return initializers;
        };
        let mut previous: Option<usize> = None;
        for action in actions {
            let span = self
                .program
                .actions
                .get(*action)
                .and_then(|node| node.span());
            let Some(Action::SetGlobalVariable {
                variable, value, ..
            }) = self.program.actions.get(*action)
            else {
                continue;
            };
            let variable_position = variable.index();
            let name = self
                .program
                .global_variables
                .get(*variable)
                .map(|variable| variable.name.clone())
                .unwrap_or_default();
            if let Some(previous_position) = previous {
                if variable_position <= previous_position {
                    self.issue(
                        "unsupported-init-rule",
                        format!(
                            "initializer rule Sets '{name}' out of global table order; \
                             the frontend synthesizes initializers in declaration order"
                        ),
                        span,
                    );
                }
            }
            previous = Some(variable_position);
            initializers.insert(variable_position, *value);
        }
        initializers
    }

    fn collect_player_initializers(
        &mut self,
        actions: &Option<Vec<wir::ActionId>>,
    ) -> std::collections::HashMap<usize, wir::ValueId> {
        let mut initializers = std::collections::HashMap::new();
        let Some(actions) = actions else {
            return initializers;
        };
        let mut previous: Option<usize> = None;
        for action in actions {
            let span = self
                .program
                .actions
                .get(*action)
                .and_then(|node| node.span());
            let Some(Action::SetPlayerVariable {
                player,
                variable,
                value,
                ..
            }) = self.program.actions.get(*action)
            else {
                continue;
            };
            if !self.is_event_player(*player) {
                self.issue(
                    "unsupported-init-rule",
                    "player initializer targets a non-event-player expression",
                    span,
                );
            }
            let variable_position = variable.index();
            let name = self
                .program
                .player_variables
                .get(*variable)
                .map(|variable| variable.name.clone())
                .unwrap_or_default();
            if let Some(previous_position) = previous {
                if variable_position <= previous_position {
                    self.issue(
                        "unsupported-init-rule",
                        format!(
                            "initializer rule Sets '{name}' out of player table order; \
                             the frontend synthesizes initializers in declaration order"
                        ),
                        span,
                    );
                }
            }
            previous = Some(variable_position);
            initializers.insert(variable_position, *value);
        }
        initializers
    }

    /// A declaration initializer: same value emission, but zero literals are
    /// spelled `0.0` because the frontend drops integer-`0` initializers
    /// (matching the reference adapter).
    fn emit_initializer(&mut self, value: wir::ValueId) {
        let Some(node) = self.program.values.get(value) else {
            self.issue("unsupported-dangling", "dangling value id", None);
            return;
        };
        if let Value::Number { value: number, .. } = &node.value {
            if *number == 0.0 {
                self.out.push_str("0.0");
                return;
            }
        }
        self.emit_value(value);
    }

    /// The OPY declaration `globalvar name = value` cannot carry an explicit
    /// slot, so the frontend re-lowering assigns the lowest free slot. An
    /// initializer-bearing global is only representable when that slot equals
    /// its WIR index; otherwise the reconstructed table would differ.
    fn check_initializer_slot(
        &mut self,
        initializers: &std::collections::HashMap<usize, wir::ValueId>,
    ) {
        let mut taken: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for (position, variable) in self.program.global_variables.iter().enumerate() {
            if initializers.contains_key(&position) {
                let mut next_free = 0u32;
                while taken.contains(&next_free) {
                    next_free += 1;
                }
                if next_free != variable.index {
                    self.issues.push(ReconstructIssue {
                        code: "unsupported-indexed-initializer",
                        message: format!(
                            "initializer-bearing global '{}' occupies slot {} but the \
                             OPY `globalvar name = value` form assigns the lowest free \
                             slot ({}) on re-lowering",
                            variable.name, variable.index, next_free
                        ),
                        span: variable.span,
                    });
                }
                taken.insert(next_free);
            } else {
                taken.insert(variable.index);
            }
        }
    }

    fn emit_actions(&mut self, actions: &[wir::ActionId], level: usize) {
        for action in actions {
            self.emit_action(*action, level);
        }
    }

    fn indent(level: usize) -> String {
        "    ".repeat(level)
    }

    fn emit_action(&mut self, id: wir::ActionId, level: usize) {
        let Some(node) = self.program.actions.get(id) else {
            self.issue("unsupported-dangling", "dangling action id", None);
            return;
        };
        let span = node.span();
        let indent = Self::indent(level);
        match node {
            Action::SetGlobalVariable {
                variable, value, ..
            } => {
                let variable_id = *variable;
                let Some(variable) = self.program.global_variables.get(variable_id) else {
                    self.issue("unsupported-dangling", "dangling global variable id", span);
                    return;
                };
                if self.set_has_modify_pattern(*value, variable_id.index(), true) {
                    self.issue(
                        "unsupported-set-binary",
                        format!(
                            "Set Global Variable('{}', <binary over the same variable>) \
                             re-lowers to a Modify action; emit the modify form",
                            variable.name
                        ),
                        span,
                    );
                    return;
                }
                self.out.push_str(&indent);
                self.out.push_str(&variable.name);
                self.out.push_str(" = ");
                self.emit_value(*value);
                self.out.push('\n');
            }
            Action::ModifyGlobalVariable {
                variable,
                op,
                value,
                ..
            } => {
                let Some(variable) = self.program.global_variables.get(*variable) else {
                    self.issue("unsupported-dangling", "dangling global variable id", span);
                    return;
                };
                self.emit_modify(level, &variable.name, *op, *value, span);
            }
            Action::SetPlayerVariable {
                player,
                variable,
                value,
                ..
            } => {
                let variable_id = *variable;
                let Some(variable) = self.program.player_variables.get(variable_id) else {
                    self.issue("unsupported-dangling", "dangling player variable id", span);
                    return;
                };
                if !self.is_event_player(*player) {
                    self.issue(
                        "unsupported-arbitrary-player-target",
                        "Set Player Variable targets a non-event-player expression; \
                         the OPY surface only exposes eventPlayer.member"
                            .to_string(),
                        span,
                    );
                    return;
                }
                if self.set_has_modify_pattern(*value, variable_id.index(), false) {
                    self.issue(
                        "unsupported-set-binary",
                        format!(
                            "Set Player Variable('{}', <binary over the same variable>) \
                             re-lowers to a Modify action; emit the modify form",
                            variable.name
                        ),
                        span,
                    );
                    return;
                }
                self.out.push_str(&indent);
                self.out.push_str("eventPlayer.");
                self.out.push_str(&variable.name);
                self.out.push_str(" = ");
                self.emit_value(*value);
                self.out.push('\n');
            }
            Action::ModifyPlayerVariable {
                player,
                variable,
                op,
                value,
                ..
            } => {
                let Some(variable) = self.program.player_variables.get(*variable) else {
                    self.issue("unsupported-dangling", "dangling player variable id", span);
                    return;
                };
                if !self.is_event_player(*player) {
                    self.issue(
                        "unsupported-arbitrary-player-target",
                        "Modify Player Variable targets a non-event-player expression; \
                         the OPY surface only exposes eventPlayer.member"
                            .to_string(),
                        span,
                    );
                    return;
                }
                self.emit_modify(
                    level,
                    &format!("eventPlayer.{}", variable.name),
                    *op,
                    *value,
                    span,
                );
            }
            Action::AssignMember { span, .. } => {
                self.issue(
                    "unsupported-member-assignment",
                    "dynamic member assignments are outside the OPY reconstruction surface",
                    *span,
                );
            }
            Action::CallSubroutine {
                subroutine, span, ..
            } => {
                let Some(subroutine) = self.program.subroutines.get(*subroutine) else {
                    self.issue("unsupported-dangling", "dangling subroutine id", *span);
                    return;
                };
                self.out.push_str(&indent);
                self.out.push_str(&subroutine.name);
                self.out.push_str("()\n");
            }
            Action::If {
                branches,
                else_body,
                span,
            } => {
                for (index, branch) in branches.iter().enumerate() {
                    let keyword = if index == 0 { "if" } else { "elif" };
                    self.out.push_str(&indent);
                    self.out.push_str(keyword);
                    self.out.push(' ');
                    self.emit_value(branch.condition);
                    self.out.push_str(":\n");
                    self.emit_actions(&branch.body, level + 1);
                }
                if let Some(else_body) = else_body {
                    self.out.push_str(&indent);
                    self.out.push_str("else:\n");
                    self.emit_actions(else_body, level + 1);
                }
                let _ = span;
            }
            Action::While {
                condition,
                body,
                span,
            } => {
                self.out.push_str(&indent);
                self.out.push_str("while ");
                self.emit_value(*condition);
                self.out.push_str(":\n");
                self.emit_actions(body, level + 1);
                let _ = span;
            }
            Action::ForGlobalVariable {
                variable,
                start,
                stop,
                step,
                body,
                span,
                ..
            } => {
                let Some(variable) = self.program.global_variables.get(*variable) else {
                    self.issue("unsupported-dangling", "dangling loop variable id", *span);
                    return;
                };
                self.out.push_str(&indent);
                self.out.push_str("for ");
                self.out.push_str(&variable.name);
                self.out.push_str(" in range(");
                self.emit_value(*start);
                self.out.push_str(", ");
                self.emit_value(*stop);
                self.out.push_str(", ");
                self.emit_value(*step);
                self.out.push_str("):\n");
                self.emit_actions(body, level + 1);
            }
            Action::ForPlayerVariable { span, .. } => {
                self.issue(
                    "unsupported-per-player-loop",
                    "For Player Variable is outside the reconstruction surface \
                     (the OPY `for` form binds a global variable)",
                    *span,
                );
            }
            Action::Debug { value, span } => {
                self.out.push_str(&indent);
                self.out.push_str("debug(");
                self.emit_value(*value);
                self.out.push_str(")\n");
                let _ = span;
            }
            Action::Print { message, span } => {
                self.out.push_str(&indent);
                self.out.push_str("print(");
                self.emit_value(*message);
                self.out.push_str(")\n");
                let _ = span;
            }
            Action::Call { name, args, span } => {
                self.emit_call_action(name, args, &indent, *span);
            }
        }
    }

    /// `x = x <op> v` (or the player form) re-lowers to a Modify action, so a
    /// Set whose value matches the pattern cannot be reconstructed as a Set.
    fn set_has_modify_pattern(
        &self,
        value: wir::ValueId,
        variable_index: usize,
        global: bool,
    ) -> bool {
        let Some(node) = self.program.values.get(value) else {
            return false;
        };
        let Value::Call { name, args } = &node.value else {
            return false;
        };
        if !matches!(name.as_str(), "+" | "-" | "*" | "/" | "%" | "**") {
            return false;
        }
        if args.len() != 2 {
            return false;
        }
        args.iter().any(|operand| {
            let Some(node) = self.program.values.get(*operand) else {
                return false;
            };
            if global {
                matches!(node.value, Value::GlobalVariable(id) if id.index() == variable_index)
            } else {
                matches!(
                    node.value,
                    Value::PlayerVariable { variable: id, .. } if id.index() == variable_index
                )
            }
        })
    }

    /// Whether a value node is the event-player pseudo-symbol.
    fn is_event_player(&self, value: wir::ValueId) -> bool {
        matches!(
            self.program.values.get(value).map(|node| &node.value),
            Some(Value::EventPlayer)
        )
    }

    fn emit_modify(
        &mut self,
        level: usize,
        name: &str,
        op: ModifyOp,
        value: wir::ValueId,
        span: Option<Span>,
    ) {
        let indent = Self::indent(level);
        match op {
            ModifyOp::AppendToArray => {
                self.out.push_str(&indent);
                self.out.push_str(name);
                self.out.push_str(".append(");
                self.emit_value(value);
                self.out.push_str(")\n");
            }
            ModifyOp::RemoveFromArray => {
                self.issue(
                    "unsupported-modify-op",
                    "Modify ... Remove From Array is outside the reconstruction surface \
                     (the OPY surface has no remove-from-array form)",
                    span,
                );
            }
            ModifyOp::RemoveFromArrayByIndex => {
                self.issue(
                    "unsupported-modify-op",
                    "Modify ... Remove From Array By Index is outside the reconstruction \
                     surface (the OPY surface has no indexed remove-from-array form)",
                    span,
                );
            }
            ModifyOp::Add
            | ModifyOp::Subtract
            | ModifyOp::Multiply
            | ModifyOp::Divide
            | ModifyOp::Modulo
            | ModifyOp::RaiseToPower => {
                let operator = match op {
                    ModifyOp::Add => "+",
                    ModifyOp::Subtract => "-",
                    ModifyOp::Multiply => "*",
                    ModifyOp::Divide => "/",
                    ModifyOp::Modulo => "%",
                    ModifyOp::RaiseToPower => "**",
                    _ => unreachable!(),
                };
                self.out.push_str(&indent);
                self.out.push_str(name);
                self.out.push_str(" = ");
                self.out.push_str(name);
                self.out.push(' ');
                self.out.push_str(operator);
                self.out.push(' ');
                self.emit_value(value);
                self.out.push('\n');
            }
        }
    }

    /// A generic or member action call in statement position.
    fn emit_call_action(
        &mut self,
        name: &str,
        args: &[wir::ValueId],
        indent: &str,
        span: Option<Span>,
    ) {
        if DEDICATED_ACTION_NAMES.contains(&name) {
            self.issue(
                "unsupported-action-call",
                format!(
                    "action call '{name}' is lowered to a dedicated WIR node by the \
                     OPY frontend and has no reconstructible call form"
                ),
                span,
            );
            return;
        }
        let Some(entry) = self.manifest.resolve_function(name) else {
            match self.manifest.resolve_member(name) {
                Some(entry) if entry.kind.is_action() => {
                    self.emit_member_call(entry, args, indent, span);
                }
                Some(_) => {
                    self.issue(
                        "unsupported-action-call",
                        format!(
                            "member value '{name}' cannot be emitted as an action on \
                             the reconstruction surface"
                        ),
                        span,
                    );
                }
                None => {
                    self.issue(
                        "unsupported-action-call",
                        format!(
                            "action call '{name}' has no OPY source form on the \
                             reconstruction surface"
                        ),
                        span,
                    );
                }
            }
            return;
        };
        if !entry.kind.is_action() {
            self.issue(
                "unsupported-action-call",
                format!(
                    "value function '{name}' cannot be emitted as an action on \
                     the reconstruction surface"
                ),
                span,
            );
            return;
        }
        if args.is_empty() && self.subroutine_names.contains(name) {
            self.issue(
                "unsupported-action-call",
                format!(
                    "action '{name}' with no arguments is ambiguous with a subroutine \
                     of the same name on the OPY surface"
                ),
                span,
            );
            return;
        }
        self.out.push_str(indent);
        self.emit_manifest_call(entry, args, false, span);
        self.out.push('\n');
    }

    /// Emit a manifest function call with explicit full-arity arguments, no
    /// indent and no trailing newline (the caller frames the line). The OPY
    /// frontend fills declared defaults at recompile time, so any WIR call
    /// that omits a defaulted or required parameter cannot be reconstructed
    /// identically and is rejected.
    fn emit_manifest_call(
        &mut self,
        entry: &Function,
        args: &[wir::ValueId],
        member: bool,
        span: Option<Span>,
    ) {
        let (receiver, params) = if member {
            match args.split_first() {
                Some((receiver, rest)) => (Some(*receiver), rest),
                None => {
                    self.issue(
                        "unsupported-invalid-arity",
                        format!("member '{}' requires a receiver argument", entry.id),
                        span,
                    );
                    return;
                }
            }
        } else {
            (None, args)
        };
        let name = entry.id.as_str();
        if params.len() > entry.params.len() {
            self.issue(
                "unsupported-invalid-arity",
                format!(
                    "{} '{}' expects at most {} arguments but the WIR carries {}",
                    kind_label(entry.kind),
                    name,
                    entry.params.len(),
                    params.len()
                ),
                span,
            );
            return;
        }
        // Every parameter beyond the provided arguments must be omittable
        // (`optional`). A required parameter (with or without a declared
        // default) cannot be omitted: the OPY frontend would reject it or
        // fill its default, changing the recompiled WIR.
        for (_index, param) in entry.params.iter().enumerate().skip(params.len()) {
            if !param.optional {
                self.issue(
                    "unsupported-missing-argument",
                    format!(
                        "{} '{}' omits parameter '{}'; the OPY frontend would \
                         reject or default-fill it and change the recompiled WIR",
                        kind_label(entry.kind),
                        name,
                        param.name
                    ),
                    span,
                );
            }
        }

        if let Some(receiver) = receiver {
            self.emit_value(receiver);
            self.out.push('.');
        }
        self.out.push_str(name);
        self.out.push('(');
        // Cross-check through the Workshop catalog: a manifest entry with a
        // declared `catalogId` must resolve there under the matching kind and
        // the reconstruction locale (mirroring the manifest's own catalog
        // cross-check test), so the reconstruction identity layer never
        // drifts from the catalog.
        if let Some(catalog_id) = &entry.catalog_id {
            let expected_kind = match entry.kind {
                FunctionKind::Action | FunctionKind::MemberAction => {
                    workshop_rs::catalog::Kind::Action
                }
                FunctionKind::Value | FunctionKind::MemberValue => {
                    workshop_rs::catalog::Kind::Value
                }
            };
            if self
                .catalog
                .spelling(expected_kind, self.locale, catalog_id)
                .is_none()
            {
                self.issue(
                    "catalog-error",
                    format!(
                        "manifest entry '{}' links catalogId '{catalog_id}' which is \
                         missing from the Workshop catalog",
                        entry.id
                    ),
                    span,
                );
            }
        }
        for (index, arg) in params.iter().enumerate() {
            if index > 0 {
                self.out.push_str(", ");
            }
            self.check_param_argument(entry, index, *arg, span);
            self.emit_value(*arg);
        }
        self.out.push(')');
    }

    /// A member call: `receiver.name(args...)`.
    fn emit_member_call(
        &mut self,
        entry: &Function,
        args: &[wir::ValueId],
        indent: &str,
        span: Option<Span>,
    ) {
        self.out.push_str(indent);
        self.emit_manifest_call(entry, args, true, span);
        self.out.push('\n');
    }

    /// Validate a provided argument against its manifest parameter: enum
    /// domains are enforced (like the frontend) and `variable`-required
    /// parameters must be variable references.
    fn check_param_argument(
        &mut self,
        entry: &Function,
        index: usize,
        arg: wir::ValueId,
        span: Option<Span>,
    ) {
        let Some(param) = entry.params.get(index) else {
            return;
        };
        let Some(node) = self.program.values.get(arg) else {
            return;
        };
        if let Some(domain) = &param.domain {
            match &node.value {
                Value::Enum { value_type, value } if value_type == domain => {
                    if !self.enum_member_in_domain(domain, value) {
                        self.issue(
                            "unsupported-enum-member",
                            format!(
                                "argument {} of '{}' uses enum member '{domain}.{value}' \
                                 which is outside the manifest's declared domain",
                                index + 1,
                                entry.id
                            ),
                            span,
                        );
                    }
                }
                Value::Enum { value_type, .. } => {
                    self.issue(
                        "unsupported-enum-domain-mismatch",
                        format!(
                            "argument {} of '{}' expects enum domain '{domain}' but \
                             the WIR carries '{value_type}'",
                            index + 1,
                            entry.id
                        ),
                        span,
                    );
                }
                _ => {
                    self.issue(
                        "unsupported-enum-domain-mismatch",
                        format!(
                            "argument {} of '{}' expects an enum member of domain \
                             '{domain}'",
                            index + 1,
                            entry.id
                        ),
                        span,
                    );
                }
            }
        }
        if param.variable {
            let is_variable = matches!(
                node.value,
                Value::GlobalVariable(_) | Value::PlayerVariable { .. }
            );
            if !is_variable {
                self.issue(
                    "unsupported-invalid-argument",
                    format!(
                        "argument {} of '{}' must be a variable reference",
                        index + 1,
                        entry.id
                    ),
                    span,
                );
            }
        }
    }

    fn enum_member_in_domain(&self, domain: &str, member: &str) -> bool {
        self.manifest
            .enum_domain(domain)
            .is_some_and(|domain| domain.members.iter().any(|candidate| candidate == member))
    }

    // ---- value emission ----

    fn emit_value(&mut self, id: wir::ValueId) {
        let Some(node) = self.program.values.get(id) else {
            self.issue("unsupported-dangling", "dangling value id", None);
            return;
        };
        match &node.value {
            Value::Number { value, .. } => {
                if !value.is_finite() {
                    self.issue(
                        "unsupported-non-finite-number",
                        format!("non-finite number literal '{value}' has no OPY spelling"),
                        node.span,
                    );
                } else if *value < 0.0 {
                    self.issue(
                        "unsupported-negative-number",
                        format!(
                            "negative number literal '{}' has no OPY literal form \
                             (the lexer has no negative-number token)",
                            wright_ir::format::format_number(*value)
                        ),
                        node.span,
                    );
                } else {
                    self.out.push_str(&wright_ir::format::format_number(*value));
                }
            }
            Value::String(value) => self.emit_string_literal(value),
            Value::LocalizedString(value) => {
                self.issue(
                    "unsupported-localized-string",
                    format!("localized Workshop preset string '{value}' has no OPY source representation"),
                    node.span,
                );
            }
            Value::Bool(value) => {
                self.out.push_str(if *value { "true" } else { "false" });
            }
            Value::Null => {
                self.out.push_str("None");
            }
            Value::Array(elements) => {
                self.out.push('[');
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        self.out.push_str(", ");
                    }
                    self.emit_value(*element);
                }
                self.out.push(']');
            }
            Value::Vector { x, y, z } => {
                self.out.push_str("vect(");
                self.emit_value(*x);
                self.out.push_str(", ");
                self.emit_value(*y);
                self.out.push_str(", ");
                self.emit_value(*z);
                self.out.push(')');
            }
            Value::Enum { value_type, value } => {
                self.emit_enum(value_type, value, node.span);
            }
            Value::GlobalVariable(variable) => {
                let Some(variable) = self.program.global_variables.get(*variable) else {
                    self.issue(
                        "unsupported-dangling",
                        "dangling global variable id",
                        node.span,
                    );
                    return;
                };
                self.out.push_str(&variable.name);
            }
            Value::PlayerVariable { player, variable } => {
                if !self.is_event_player(*player) {
                    self.issue(
                        "unsupported-arbitrary-player-target",
                        "a player-variable access on a non-event-player expression is \
                         outside the reconstruction surface (only eventPlayer.member \
                         is representable)",
                        node.span,
                    );
                    return;
                }
                let Some(variable) = self.program.player_variables.get(*variable) else {
                    self.issue(
                        "unsupported-dangling",
                        "dangling player variable id",
                        node.span,
                    );
                    return;
                };
                self.out.push_str("eventPlayer.");
                self.out.push_str(&variable.name);
            }
            Value::Subroutine(_) => {
                self.issue(
                    "unsupported-subroutine-value",
                    "subroutine values are outside the OPY reconstruction surface",
                    node.span,
                );
            }
            Value::EventPlayer => {
                self.out.push_str("eventPlayer");
            }
            Value::Call { name, args } => {
                self.emit_value_call(name, args, node.span);
            }
        }
    }

    fn emit_enum(&mut self, value_type: &str, value: &str, span: Option<Span>) {
        let Some(domain) = self.manifest.enum_domain(value_type) else {
            self.issue(
                "unsupported-enum-domain",
                format!(
                    "enum domain '{value_type}' is outside the manifest's declared \
                     reconstruction surface"
                ),
                span,
            );
            return;
        };
        if !domain.members.iter().any(|member| member == value) {
            self.issue(
                "unsupported-enum-member",
                format!(
                    "enum member '{value_type}.{value}' is outside the manifest's \
                     declared domain"
                ),
                span,
            );
            return;
        }
        self.out.push_str(value_type);
        self.out.push('.');
        self.out.push_str(value);
    }

    fn emit_value_call(&mut self, name: &str, args: &[wir::ValueId], span: Option<Span>) {
        // Binary and unary operator calls keep their source spelling.
        if BINARY_OPS.contains(&name) && args.len() == 2 {
            self.out.push('(');
            self.emit_value(args[0]);
            self.out.push(' ');
            self.out.push_str(name);
            self.out.push(' ');
            self.emit_value(args[1]);
            self.out.push(')');
            return;
        }
        if name == "not" && args.len() == 1 {
            self.out.push_str("(not ");
            self.emit_value(args[0]);
            self.out.push(')');
            return;
        }
        if name == "-" && args.len() == 1 {
            self.out.push_str("(-");
            self.emit_value(args[0]);
            self.out.push(')');
            return;
        }
        // The `format` special form: `"text".format(args...)`.
        if name == "format" {
            let Some(first) = args.first() else {
                self.issue(
                    "unsupported-value-call",
                    "format call without a receiver is outside the reconstruction surface",
                    span,
                );
                return;
            };
            let Some(Value::String(text)) = self.program.values.get(*first).map(|node| &node.value)
            else {
                self.issue(
                    "unsupported-value-call",
                    "format call without a string receiver is outside the \
                     reconstruction surface",
                    span,
                );
                return;
            };
            self.emit_string_literal(text);
            self.out.push_str(".format(");
            for (index, arg) in args.iter().skip(1).enumerate() {
                if index > 0 {
                    self.out.push_str(", ");
                }
                self.emit_value(*arg);
            }
            self.out.push(')');
            return;
        }
        if DEDICATED_VALUE_NAMES.contains(&name) {
            self.issue(
                "unsupported-value-call",
                format!(
                    "value call '{name}' is lowered to a dedicated WIR node by the \
                     OPY frontend and has no reconstructible call form"
                ),
                span,
            );
            return;
        }
        let Some(entry) = self.manifest.resolve_function(name) else {
            match self.manifest.resolve_member(name) {
                Some(entry) if entry.kind.is_value() => {
                    self.emit_manifest_call(entry, args, true, span);
                }
                Some(_) => {
                    self.issue(
                        "unsupported-value-call",
                        format!(
                            "member action '{name}' cannot be emitted as a value on \
                             the reconstruction surface"
                        ),
                        span,
                    );
                }
                None => {
                    self.issue(
                        "unsupported-value-call",
                        format!(
                            "value call '{name}' has no OPY source form on the \
                             reconstruction surface"
                        ),
                        span,
                    );
                }
            }
            return;
        };
        if !entry.kind.is_value() {
            self.issue(
                "unsupported-value-call",
                format!(
                    "action function '{name}' cannot be emitted as a value on the \
                     reconstruction surface"
                ),
                span,
            );
            return;
        }
        if entry.context.is_some() {
            self.issue(
                "unsupported-value-call",
                format!(
                    "value call '{name}' is only valid as a for-loop iterable on \
                     the OPY surface"
                ),
                span,
            );
            return;
        }
        self.emit_manifest_call(entry, args, false, span);
    }

    fn emit_string_literal(&mut self, value: &str) {
        self.out.push('"');
        for ch in value.chars() {
            match ch {
                '\\' => self.out.push_str("\\\\"),
                '"' => self.out.push_str("\\\""),
                '\n' => self.out.push_str("\\n"),
                '\t' => self.out.push_str("\\t"),
                '\r' => self.out.push_str("\\r"),
                other => self.out.push(other),
            }
        }
        self.out.push('"');
    }
}

fn kind_label(kind: FunctionKind) -> &'static str {
    match kind {
        FunctionKind::Action => "action",
        FunctionKind::Value => "value",
        FunctionKind::MemberAction => "member action",
        FunctionKind::MemberValue => "member value",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workshop_rs::source::{Position, SourceFile};
    use workshop_rs::wir::{Program, ValueNode};

    fn catalog() -> Catalog {
        Catalog::builtin().unwrap()
    }

    fn manifest() -> &'static Manifest {
        Manifest::builtin().unwrap()
    }

    fn en() -> Locale {
        Locale::new("en-US")
    }

    fn span() -> Span {
        Span::new(
            wright_ir::ids::Id::from_index(0),
            Position::new(1, 1),
            Position::new(1, 1),
        )
    }

    fn value(program: &mut Program, value: Value) -> wir::ValueId {
        program.values.push(ValueNode::new(value, Some(span())))
    }

    fn number(program: &mut Program, number: f64) -> wir::ValueId {
        value(
            program,
            Value::Number {
                value: number,
                text: wright_ir::format::format_number(number),
            },
        )
    }

    fn global(program: &mut Program, name: &str) -> (wir::GlobalVarId, wir::ValueId) {
        let id = program.global_variables.push(wir::WorkshopVariable {
            name: name.to_string(),
            index: program.global_variables.len() as u32,
            span: None,
            name_span: None,
        });
        let read = value(program, Value::GlobalVariable(id));
        (id, read)
    }

    fn player(program: &mut Program, name: &str) -> (wir::PlayerVarId, wir::ValueId) {
        let id = program.player_variables.push(wir::WorkshopVariable {
            name: name.to_string(),
            index: program.player_variables.len() as u32,
            span: None,
            name_span: None,
        });
        let player = value(program, Value::EventPlayer);
        let read = value(
            program,
            Value::PlayerVariable {
                player,
                variable: id,
            },
        );
        (id, read)
    }

    fn event_player(program: &mut Program) -> wir::ValueId {
        value(program, Value::EventPlayer)
    }

    fn global_rule(program: &mut Program, name: &str, actions: Vec<wir::ActionId>) {
        program.rules.push(wir::Rule {
            name: name.to_string(),
            span: None,
            name_span: None,
            disabled: false,
            event: Event::Global,
            conditions: Vec::new(),
            actions,
        });
    }

    fn emit(program: &Program) -> Result<String, ReconstructError> {
        reconstruct_with(program, manifest(), &catalog(), &en())
    }

    /// The shipped end-to-end path for a WIR program built without a
    /// Workshop source: reconstruct → native frontend → WIR, asserted
    /// equivalent.
    fn round_trip(program: &Program) {
        let source = emit(program).expect("reconstruction succeeds");
        let recompiled = recompile(&source);
        assert!(
            workshop_rs::roundtrip::equivalent(program, &recompiled),
            "recompiled WIR must be equivalent to the input:\n{source}"
        );
    }

    /// Recompile reconstructed OPY through the shipped native frontend.
    fn recompile(source: &str) -> workshop_rs::wir::Program {
        let hir = crate::compile(source, "reconstructed.opy", std::path::Path::new(""))
            .expect("the native frontend accepts the reconstructed OPY");
        wright_ir::lower::lower(&hir.to_ir().expect("converts to internal HIR"))
            .expect("lowers to WIR")
    }

    fn file_registry(program: &mut Program) {
        program
            .files
            .push(SourceFile::new("reconstructed.opy".to_string()));
    }

    #[test]
    fn reconstructs_a_basic_program_deterministically() {
        let mut program = Program::default();
        file_registry(&mut program);
        let (score, _score_read) = global(&mut program, "score");
        let (_other, other_read) = global(&mut program, "other");
        let (_, has_started_read) = player(&mut program, "hasStarted");
        let one = number(&mut program, 1.0);
        let sum = value(
            &mut program,
            Value::Call {
                name: "+".to_string(),
                args: vec![other_read, one],
            },
        );
        let action = program.actions.push(Action::SetGlobalVariable {
            variable: score,
            value: sum,
            span: None,
            target_span: None,
        });
        let print_action = program.actions.push(Action::Print {
            message: has_started_read,
            span: None,
        });
        global_rule(&mut program, "main", vec![action, print_action]);

        let first = emit(&program).expect("reconstructs");
        let second = emit(&program).expect("reconstructs");
        assert_eq!(first, second, "reconstruction must be byte-stable");
        // Structural assertions on the reconstructed source (no golden
        // bytes): the declarations, the event, the binary expression, and
        // the print statement all appear in their OPY source forms.
        assert!(first.starts_with("globalvar score 0\nglobalvar other 1\nplayervar hasStarted 0"));
        assert!(first.contains("score = (other + 1)"));
        assert!(first.contains("print(eventPlayer.hasStarted)"));
        // `equivalent` does not cover the dedicated Debug/Print nodes, so
        // the round-trip is checked on the Set actions and the print
        // recompilation is verified structurally on the recompiled WIR.
        let mut comparable = program.clone();
        let rule_id = wright_ir::ids::Id::from_index(0);
        comparable.rules.get_mut(rule_id).unwrap().actions.pop();
        round_trip(&comparable);
        let recompiled = recompile(&first);
        assert!(
            recompiled.dump().contains("print eventPlayer.hasStarted"),
            "print recompiles to the Print node:\n{}",
            recompiled.dump()
        );
    }

    #[test]
    fn emits_debug_arrays_vectors_and_format() {
        let mut program = Program::default();
        file_registry(&mut program);
        let (result, result_read) = global(&mut program, "result");
        let one = number(&mut program, 1.0);
        let two = number(&mut program, 2.0);
        let three = number(&mut program, 3.0);
        let array = value(&mut program, Value::Array(vec![one, two]));
        let vector = value(
            &mut program,
            Value::Vector {
                x: one,
                y: two,
                z: three,
            },
        );
        let text = value(&mut program, Value::String("x: {0}".to_string()));
        let formatted = value(
            &mut program,
            Value::Call {
                name: "format".to_string(),
                args: vec![text, result_read],
            },
        );
        let actions = vec![
            program.actions.push(Action::SetGlobalVariable {
                variable: result,
                value: array,
                span: None,
                target_span: None,
            }),
            program.actions.push(Action::SetGlobalVariable {
                variable: result,
                value: vector,
                span: None,
                target_span: None,
            }),
            program.actions.push(Action::SetGlobalVariable {
                variable: result,
                value: formatted,
                span: None,
                target_span: None,
            }),
            program.actions.push(Action::Debug {
                value: result_read,
                span: None,
            }),
        ];
        global_rule(&mut program, "main", actions);
        let source = emit(&program).expect("reconstructs");
        assert!(source.contains("result = [1, 2]"));
        assert!(source.contains("result = vect(1, 2, 3)"));
        assert!(source.contains("result = \"x: {0}\".format(result)"));
        assert!(source.contains("debug(result)"));
        // `equivalent` does not cover the dedicated Debug/Print nodes, so the
        // equivalent part (Sets) is checked on a Debug-free copy and the
        // debug recompilation is verified structurally.
        let mut comparable = program.clone();
        let rule_id = wright_ir::ids::Id::from_index(0);
        comparable.rules.get_mut(rule_id).unwrap().actions.pop();
        round_trip(&comparable);
        let recompiled = recompile(&source);
        assert!(
            recompiled.dump().contains("debug result"),
            "debug recompiles to the Debug node:\n{}",
            recompiled.dump()
        );
    }

    #[test]
    fn emits_manifest_calls_with_full_arity() {
        let mut program = Program::default();
        file_registry(&mut program);
        let (result, result_read) = global(&mut program, "result");
        let receiver = event_player(&mut program);
        let two = number(&mut program, 2.0);
        let team_all = value(
            &mut program,
            Value::Enum {
                value_type: "Team".to_string(),
                value: "ALL".to_string(),
            },
        );
        let los_off = value(
            &mut program,
            Value::Enum {
                value_type: "LosCheck".to_string(),
                value: "OFF".to_string(),
            },
        );
        let radius = value(
            &mut program,
            Value::Call {
                name: "getPlayersInRadius".to_string(),
                args: vec![result_read, two, team_all, los_off],
            },
        );
        let wait_time = number(&mut program, 0.016);
        let wait_behavior = value(
            &mut program,
            Value::Enum {
                value_type: "Wait".to_string(),
                value: "IGNORE_CONDITION".to_string(),
            },
        );
        let wait = program.actions.push(Action::Call {
            name: "wait".to_string(),
            args: vec![wait_time, wait_behavior],
            span: None,
        });
        let set_radius = program.actions.push(Action::SetGlobalVariable {
            variable: result,
            value: radius,
            span: None,
            target_span: None,
        });
        let move_speed = number(&mut program, 100.0);
        let member_call = program.actions.push(Action::Call {
            name: "setMoveSpeed".to_string(),
            args: vec![receiver, move_speed],
            span: None,
        });
        global_rule(&mut program, "main", vec![set_radius, member_call, wait]);
        let source = emit(&program).expect("reconstructs");
        assert!(source.contains("getPlayersInRadius(result, 2, Team.ALL, LosCheck.OFF)"));
        assert!(source.contains("eventPlayer.setMoveSpeed(100)"));
        assert!(source.contains("wait(0.016, Wait.IGNORE_CONDITION)"));
        round_trip(&program);
    }

    #[test]
    fn emits_modify_forms() {
        let mut program = Program::default();
        file_registry(&mut program);
        let (score, score_read) = global(&mut program, "score");
        let one = number(&mut program, 1.0);
        let five = number(&mut program, 5.0);
        let actions = vec![
            program.actions.push(Action::ModifyGlobalVariable {
                variable: score,
                op: ModifyOp::Add,
                value: one,
                span: None,
                target_span: None,
            }),
            program.actions.push(Action::ModifyGlobalVariable {
                variable: score,
                op: ModifyOp::AppendToArray,
                value: five,
                span: None,
                target_span: None,
            }),
            program.actions.push(Action::ModifyGlobalVariable {
                variable: score,
                op: ModifyOp::RaiseToPower,
                value: score_read,
                span: None,
                target_span: None,
            }),
        ];
        global_rule(&mut program, "main", actions);
        let source = emit(&program).expect("reconstructs");
        assert!(source.contains("score = score + 1"));
        assert!(source.contains("score.append(5)"));
        assert!(source.contains("score = score ** score"));
        round_trip(&program);
    }

    #[test]
    fn reconstructs_initializer_rules_and_defs() {
        let mut program = Program::default();
        file_registry(&mut program);
        let (score, _score_read) = global(&mut program, "score");
        let (points, _points_read) = global(&mut program, "points");
        let (kills, _) = player(&mut program, "kills");
        let (has_started, _) = player(&mut program, "hasStarted");

        // Leading "Initialize global variables" rule: score = 5, points = 0.
        let init_value = number(&mut program, 5.0);
        let zero_value = number(&mut program, 0.0);
        let one_more = number(&mut program, 1.0);
        let init_actions = vec![
            program.actions.push(Action::SetGlobalVariable {
                variable: score,
                value: init_value,
                span: None,
                target_span: None,
            }),
            program.actions.push(Action::SetGlobalVariable {
                variable: points,
                value: zero_value,
                span: None,
                target_span: None,
            }),
        ];
        program.rules.push(wir::Rule {
            name: "Initialize global variables".to_string(),
            span: None,
            name_span: None,
            disabled: false,
            event: Event::Global,
            conditions: Vec::new(),
            actions: init_actions,
        });
        // Leading "Initialize player variables" rule: kills = 3.
        let player_event = event_player(&mut program);
        let kills_three = number(&mut program, 3.0);
        let player_init = program.actions.push(Action::SetPlayerVariable {
            player: player_event,
            variable: kills,
            value: kills_three,
            span: None,
            target_span: None,
        });
        program.rules.push(wir::Rule {
            name: "Initialize player variables".to_string(),
            span: None,
            name_span: None,
            disabled: false,
            event: Event::EachPlayer,
            conditions: Vec::new(),
            actions: vec![player_init],
        });
        // Subroutine body.
        let sub_id = program.subroutines.push(wir::WorkshopSubroutine {
            name: "tick".to_string(),
            index: 0,
            span: None,
            name_span: None,
        });
        let sub_action = program.actions.push(Action::ModifyGlobalVariable {
            variable: score,
            op: ModifyOp::Add,
            value: one_more,
            span: None,
            target_span: None,
        });
        program.rules.push(wir::Rule {
            name: "Subroutine tick".to_string(),
            span: None,
            name_span: None,
            disabled: false,
            event: Event::Subroutine(sub_id),
            conditions: Vec::new(),
            actions: vec![sub_action],
        });
        // Normal rule.
        let set2_event = event_player(&mut program);
        let one_value = number(&mut program, 1.0);
        let set2 = program.actions.push(Action::SetPlayerVariable {
            player: set2_event,
            variable: has_started,
            value: one_value,
            span: None,
            target_span: None,
        });
        global_rule(&mut program, "main", vec![set2]);

        let source = emit(&program).expect("reconstructs");
        assert!(source.contains("globalvar score = 5"));
        assert!(source.contains("globalvar points = 0.0"));
        assert!(source.contains("playervar kills = 3"));
        assert!(source.contains("subroutine tick"));
        assert!(source.contains("def tick():"));
        round_trip(&program);
    }

    #[test]
    fn rejects_non_representable_constructs() {
        // Per-player loop.
        let mut program = Program::default();
        file_registry(&mut program);
        let (has_started, _) = player(&mut program, "hasStarted");
        let loop_player = event_player(&mut program);
        let loop_start = number(&mut program, 0.0);
        let loop_stop = number(&mut program, 3.0);
        let loop_step = number(&mut program, 1.0);
        let loop_action = program.actions.push(Action::ForPlayerVariable {
            player: loop_player,
            variable: has_started,
            start: loop_start,
            stop: loop_stop,
            step: loop_step,
            body: Vec::new(),
            span: None,
        });
        global_rule(&mut program, "loop", vec![loop_action]);
        let error = emit(&program).expect_err("per-player loop must be rejected");
        assert_eq!(error.issues[0].code, "unsupported-per-player-loop");

        // Disabled rule.
        let mut program = Program::default();
        file_registry(&mut program);
        let action = program.actions.push(Action::Call {
            name: "disableInspector".to_string(),
            args: Vec::new(),
            span: None,
        });
        program.rules.push(wir::Rule {
            name: "off".to_string(),
            span: None,
            name_span: None,
            disabled: true,
            event: Event::Global,
            conditions: Vec::new(),
            actions: vec![action],
        });
        let error = emit(&program).expect_err("disabled rules must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-disabled-rule")
        );

        // Arbitrary player target.
        let mut program = Program::default();
        file_registry(&mut program);
        let (result, _) = global(&mut program, "result");
        let (has_started, _) = player(&mut program, "hasStarted");
        let player_expr = value(&mut program, Value::GlobalVariable(result));
        let one_value = number(&mut program, 1.0);
        let set = program.actions.push(Action::SetPlayerVariable {
            player: player_expr,
            variable: has_started,
            value: one_value,
            span: None,
            target_span: None,
        });
        global_rule(&mut program, "main", vec![set]);
        let error = emit(&program).expect_err("arbitrary player target must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-arbitrary-player-target")
        );

        // Unrepresentable call name.
        let mut program = Program::default();
        file_registry(&mut program);
        let (result, _) = global(&mut program, "result");
        let (_, has_started) = player(&mut program, "hasStarted");
        let value_id = value(
            &mut program,
            Value::Call {
                name: "countOf".to_string(),
                args: vec![has_started],
            },
        );
        let set = program.actions.push(Action::SetGlobalVariable {
            variable: result,
            value: value_id,
            span: None,
            target_span: None,
        });
        global_rule(&mut program, "main", vec![set]);
        let error = emit(&program).expect_err("unknown value call must be rejected");
        assert!(error.issues.iter().any(
            |issue| issue.code == "unsupported-value-call" && issue.message.contains("countOf")
        ));

        // Invalid identifier.
        let mut program = Program::default();
        file_registry(&mut program);
        program.global_variables.push(wir::WorkshopVariable {
            name: "two words".to_string(),
            index: 0,
            span: None,
            name_span: None,
        });
        let error = emit(&program).expect_err("invalid identifier must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-name")
        );

        // Negative number.
        let mut program = Program::default();
        file_registry(&mut program);
        let (result, _) = global(&mut program, "result");
        let negative = number(&mut program, -1.0);
        let set = program.actions.push(Action::SetGlobalVariable {
            variable: result,
            value: negative,
            span: None,
            target_span: None,
        });
        global_rule(&mut program, "main", vec![set]);
        let error = emit(&program).expect_err("negative numbers must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-negative-number")
        );

        // Unknown enum domain.
        let mut program = Program::default();
        file_registry(&mut program);
        let (result, _) = global(&mut program, "result");
        let enum_value = value(
            &mut program,
            Value::Enum {
                value_type: "NotADomain".to_string(),
                value: "X".to_string(),
            },
        );
        let set = program.actions.push(Action::SetGlobalVariable {
            variable: result,
            value: enum_value,
            span: None,
            target_span: None,
        });
        global_rule(&mut program, "main", vec![set]);
        let error = emit(&program).expect_err("unknown enum domain must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-enum-domain")
        );

        // Unknown enum member.
        let mut program = Program::default();
        file_registry(&mut program);
        let (result, _) = global(&mut program, "result");
        let enum_value = value(
            &mut program,
            Value::Enum {
                value_type: "Color".to_string(),
                value: "CYAN".to_string(),
            },
        );
        let set = program.actions.push(Action::SetGlobalVariable {
            variable: result,
            value: enum_value,
            span: None,
            target_span: None,
        });
        global_rule(&mut program, "main", vec![set]);
        let error = emit(&program).expect_err("unknown enum member must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-enum-member")
        );

        // Remove-from-array modify.
        let mut program = Program::default();
        file_registry(&mut program);
        let (score, _) = global(&mut program, "score");
        let one_value = number(&mut program, 1.0);
        let modify = program.actions.push(Action::ModifyGlobalVariable {
            variable: score,
            op: ModifyOp::RemoveFromArray,
            value: one_value,
            span: None,
            target_span: None,
        });
        global_rule(&mut program, "main", vec![modify]);
        let error = emit(&program).expect_err("remove-from-array must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-modify-op")
        );

        // Missing required argument (default filling would change the WIR).
        let mut program = Program::default();
        file_registry(&mut program);
        let wait_time = number(&mut program, 1.0);
        let wait = program.actions.push(Action::Call {
            name: "wait".to_string(),
            args: vec![wait_time],
            span: None,
        });
        global_rule(&mut program, "main", vec![wait]);
        let error = emit(&program).expect_err("short wait must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-missing-argument")
        );

        // Invalid arity.
        let mut program = Program::default();
        file_registry(&mut program);
        let wait_time = number(&mut program, 1.0);
        let wait_behavior = value(
            &mut program,
            Value::Enum {
                value_type: "Wait".to_string(),
                value: "IGNORE_CONDITION".to_string(),
            },
        );
        let wait_two = number(&mut program, 2.0);
        let wait = program.actions.push(Action::Call {
            name: "wait".to_string(),
            args: vec![wait_time, wait_behavior, wait_two],
            span: None,
        });
        global_rule(&mut program, "main", vec![wait]);
        let error = emit(&program).expect_err("wait with 3 args must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-invalid-arity")
        );

        // Enum-domain mismatch at a manifest parameter.
        let mut program = Program::default();
        file_registry(&mut program);
        let wait_time = number(&mut program, 1.0);
        let yellow = value(
            &mut program,
            Value::Enum {
                value_type: "Color".to_string(),
                value: "YELLOW".to_string(),
            },
        );
        let wait = program.actions.push(Action::Call {
            name: "wait".to_string(),
            args: vec![wait_time, yellow],
            span: None,
        });
        global_rule(&mut program, "main", vec![wait]);
        let error = emit(&program).expect_err("domain mismatch must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-enum-domain-mismatch")
        );

        // Set with a same-variable binary (would re-lower to a Modify).
        let mut program = Program::default();
        file_registry(&mut program);
        let (score, score_read) = global(&mut program, "score");
        let one_value = number(&mut program, 1.0);
        let sum = value(
            &mut program,
            Value::Call {
                name: "+".to_string(),
                args: vec![score_read, one_value],
            },
        );
        let set = program.actions.push(Action::SetGlobalVariable {
            variable: score,
            value: sum,
            span: None,
            target_span: None,
        });
        global_rule(&mut program, "main", vec![set]);
        let error = emit(&program).expect_err("set-with-same-variable-binary must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-set-binary")
        );

        // Non-canonical rule order: a normal rule before a subroutine body.
        let mut program = Program::default();
        file_registry(&mut program);
        let sub_id = program.subroutines.push(wir::WorkshopSubroutine {
            name: "tick".to_string(),
            index: 0,
            span: None,
            name_span: None,
        });
        global_rule(&mut program, "normal", Vec::new());
        program.rules.push(wir::Rule {
            name: "Subroutine tick".to_string(),
            span: None,
            name_span: None,
            disabled: false,
            event: Event::Subroutine(sub_id),
            conditions: Vec::new(),
            actions: Vec::new(),
        });
        let error = emit(&program).expect_err("non-canonical rule order must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-rule-order")
        );

        // Reserved name.
        let mut program = Program::default();
        file_registry(&mut program);
        program.global_variables.push(wir::WorkshopVariable {
            name: "if".to_string(),
            index: 0,
            span: None,
            name_span: None,
        });
        let error = emit(&program).expect_err("reserved name must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-name")
        );

        // Subroutine index mismatch.
        let mut program = Program::default();
        file_registry(&mut program);
        program.subroutines.push(wir::WorkshopSubroutine {
            name: "tick".to_string(),
            index: 5,
            span: None,
            name_span: None,
        });
        let error = emit(&program).expect_err("subroutine index mismatch must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-subroutine-index")
        );

        // Unsorted global slots.
        let mut program = Program::default();
        file_registry(&mut program);
        program.global_variables.push(wir::WorkshopVariable {
            name: "a".to_string(),
            index: 5,
            span: None,
            name_span: None,
        });
        program.global_variables.push(wir::WorkshopVariable {
            name: "b".to_string(),
            index: 0,
            span: None,
            name_span: None,
        });
        let error = emit(&program).expect_err("unsorted globals must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-global-order")
        );

        // Dedicated call names.
        let mut program = Program::default();
        file_registry(&mut program);
        let (_score, _) = global(&mut program, "score");
        let one_value = number(&mut program, 1.0);
        let call = program.actions.push(Action::Call {
            name: "debug".to_string(),
            args: vec![one_value],
            span: None,
        });
        global_rule(&mut program, "main", vec![call]);
        let error = emit(&program).expect_err("debug call must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-action-call")
        );

        // Settings.
        let mut program = Program::default();
        file_registry(&mut program);
        program.settings = Some(workshop_rs::settings::Settings {
            span: None,
            children: Vec::new(),
        });
        let error = emit(&program).expect_err("settings must be rejected");
        assert!(
            error
                .issues
                .iter()
                .any(|issue| issue.code == "unsupported-settings")
        );
    }
}
