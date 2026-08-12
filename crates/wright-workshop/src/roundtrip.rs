//! Cross-language Workshop round-trip compatibility suite.
//!
//! [`round_trip`] proves `Workshop(locale) -> WIR -> Workshop(locale) -> WIR`
//! equivalence with a recorded evidence record, and [`equivalent`] compares
//! two WIR programs structurally, ignoring presentation-only differences
//! (source spans and file paths) while preserving operations, references,
//! control flow, and values.
//!
//! The v0.2 catalog supports `en-US`, so cross-locale equivalence is
//! trivially identity for now; the suite is locale-generic so additional
//! locales (a data-pipeline change) automatically extend coverage.

use wright_ir::wir;

use crate::catalog::{Catalog, Locale};
use crate::emitter;
use crate::parser;

/// A recorded round-trip result with the evidence needed for a compatibility
/// report.
#[derive(Debug, Clone, PartialEq)]
pub struct RoundTripRecord {
    /// SHA-256 of the input Workshop text.
    pub input_identity: String,
    /// The locale the text was parsed and emitted in.
    pub locale: Locale,
    /// The catalog schema version used.
    pub catalog_version: u32,
    /// Whether the input parsed.
    pub parse_ok: bool,
    /// Whether the parsed program emitted.
    pub emit_ok: bool,
    /// Whether the emitted text reparsed.
    pub reparse_ok: bool,
    /// Whether the original and round-tripped WIR are equivalent.
    pub equivalent: bool,
    /// A structured failure message, when any stage failed.
    pub error: Option<String>,
}

/// Run `Workshop -> WIR -> Workshop -> WIR` and record the evidence. The
/// record is always produced; failures are captured in its `error` field.
pub fn round_trip(input: &str, catalog: &Catalog, locale: &Locale) -> RoundTripRecord {
    let input_identity = sha256(input);
    let mut record = RoundTripRecord {
        input_identity,
        locale: locale.clone(),
        catalog_version: catalog.schema_version,
        parse_ok: false,
        emit_ok: false,
        reparse_ok: false,
        equivalent: false,
        error: None,
    };
    let first = match parser::parse(input, catalog, locale) {
        Ok(program) => program,
        Err(error) => {
            record.error = Some(error.to_string());
            return record;
        }
    };
    record.parse_ok = true;
    let emitted = match emitter::emit(&first, catalog, locale) {
        Ok(text) => text,
        Err(error) => {
            record.error = Some(error.to_string());
            return record;
        }
    };
    record.emit_ok = true;
    let second = match parser::parse(&emitted, catalog, locale) {
        Ok(program) => program,
        Err(error) => {
            record.error = Some(error.to_string());
            return record;
        }
    };
    record.reparse_ok = true;
    record.equivalent = equivalent(&first, &second);
    record
}

/// Structural equivalence of two WIR programs: identical tables, rules,
/// actions, and values, ignoring source spans and file paths.
pub fn equivalent(a: &wir::Program, b: &wir::Program) -> bool {
    let globals_a: Vec<_> = a
        .global_variables
        .iter()
        .map(|v| (v.name.as_str(), v.index))
        .collect();
    let globals_b: Vec<_> = b
        .global_variables
        .iter()
        .map(|v| (v.name.as_str(), v.index))
        .collect();
    if globals_a != globals_b {
        return false;
    }
    let players_a: Vec<_> = a
        .player_variables
        .iter()
        .map(|v| (v.name.as_str(), v.index))
        .collect();
    let players_b: Vec<_> = b
        .player_variables
        .iter()
        .map(|v| (v.name.as_str(), v.index))
        .collect();
    if players_a != players_b {
        return false;
    }
    let subs_a: Vec<_> = a
        .subroutines
        .iter()
        .map(|s| (s.name.as_str(), s.index))
        .collect();
    let subs_b: Vec<_> = b
        .subroutines
        .iter()
        .map(|s| (s.name.as_str(), s.index))
        .collect();
    if subs_a != subs_b {
        return false;
    }
    if a.rules.len() != b.rules.len() {
        return false;
    }
    for (rule_a, rule_b) in a.rules.iter().zip(b.rules.iter()) {
        if !rule_equivalent(a, b, rule_a, rule_b) {
            return false;
        }
    }
    true
}

fn rule_equivalent(
    a: &wir::Program,
    b: &wir::Program,
    left: &wir::Rule,
    right: &wir::Rule,
) -> bool {
    if left.name != right.name || left.disabled != right.disabled {
        return false;
    }
    let event_a = event_equivalent(a, b, &left.event, &right.event);
    if !event_a {
        return false;
    }
    if left.conditions.len() != right.conditions.len() {
        return false;
    }
    for (ca, cb) in left.conditions.iter().zip(right.conditions.iter()) {
        if !value_equivalent(a, b, *ca, *cb) {
            return false;
        }
    }
    if left.actions.len() != right.actions.len() {
        return false;
    }
    for (aa, ab) in left.actions.iter().zip(right.actions.iter()) {
        if !action_equivalent(a, b, *aa, *ab) {
            return false;
        }
    }
    true
}

fn event_equivalent(
    a: &wir::Program,
    b: &wir::Program,
    left: &wir::Event,
    right: &wir::Event,
) -> bool {
    match (left, right) {
        (wir::Event::Global, wir::Event::Global) => true,
        (wir::Event::EachPlayer, wir::Event::EachPlayer) => true,
        (wir::Event::Subroutine(sa), wir::Event::Subroutine(sb)) => {
            let name_a = a.subroutines.get(*sa).map(|s| s.name.as_str());
            let name_b = b.subroutines.get(*sb).map(|s| s.name.as_str());
            name_a == name_b
        }
        _ => false,
    }
}

fn action_equivalent(
    a: &wir::Program,
    b: &wir::Program,
    left: wir::ActionId,
    right: wir::ActionId,
) -> bool {
    let (Some(la), Some(rb)) = (a.actions.get(left), b.actions.get(right)) else {
        return false;
    };
    match (la, rb) {
        (
            wir::Action::SetGlobalVariable {
                variable: va,
                value: x,
                ..
            },
            wir::Action::SetGlobalVariable {
                variable: vb,
                value: y,
                ..
            },
        ) => {
            name_eq(a.global_variables.get(*va), b.global_variables.get(*vb))
                && value_equivalent(a, b, *x, *y)
        }
        (
            wir::Action::ModifyGlobalVariable {
                variable: va,
                op: oa,
                value: x,
                ..
            },
            wir::Action::ModifyGlobalVariable {
                variable: vb,
                op: ob,
                value: y,
                ..
            },
        ) => {
            name_eq(a.global_variables.get(*va), b.global_variables.get(*vb))
                && oa == ob
                && value_equivalent(a, b, *x, *y)
        }
        (
            wir::Action::SetPlayerVariable {
                player: pa,
                variable: va,
                value: x,
                ..
            },
            wir::Action::SetPlayerVariable {
                player: pb,
                variable: vb,
                value: y,
                ..
            },
        ) => {
            value_equivalent(a, b, *pa, *pb)
                && name_eq(a.player_variables.get(*va), b.player_variables.get(*vb))
                && value_equivalent(a, b, *x, *y)
        }
        (
            wir::Action::ModifyPlayerVariable {
                player: pa,
                variable: va,
                op: oa,
                value: x,
                ..
            },
            wir::Action::ModifyPlayerVariable {
                player: pb,
                variable: vb,
                op: ob,
                value: y,
                ..
            },
        ) => {
            value_equivalent(a, b, *pa, *pb)
                && name_eq(a.player_variables.get(*va), b.player_variables.get(*vb))
                && oa == ob
                && value_equivalent(a, b, *x, *y)
        }
        (
            wir::Action::CallSubroutine { subroutine: sa, .. },
            wir::Action::CallSubroutine { subroutine: sb, .. },
        ) => name_eq(a.subroutines.get(*sa), b.subroutines.get(*sb)),
        (
            wir::Action::If {
                branches: ba,
                else_body: ea,
                ..
            },
            wir::Action::If {
                branches: bb,
                else_body: eb,
                ..
            },
        ) => branches_equivalent(a, b, ba, bb) && bodies_equivalent(a, b, ea, eb),
        (
            wir::Action::While {
                condition: ca,
                body: ba,
                ..
            },
            wir::Action::While {
                condition: cb,
                body: bb,
                ..
            },
        ) => value_equivalent(a, b, *ca, *cb) && actions_equivalent(a, b, ba, bb),
        (
            wir::Action::ForGlobalVariable {
                variable: va,
                start: sa,
                stop: ea,
                step: pa,
                body: ba,
                ..
            },
            wir::Action::ForGlobalVariable {
                variable: vb,
                start: sb,
                stop: eb,
                step: pb,
                body: bb,
                ..
            },
        ) => {
            name_eq(a.global_variables.get(*va), b.global_variables.get(*vb))
                && value_equivalent(a, b, *sa, *sb)
                && value_equivalent(a, b, *ea, *eb)
                && value_equivalent(a, b, *pa, *pb)
                && actions_equivalent(a, b, ba, bb)
        }
        (
            wir::Action::Call {
                name: na, args: xa, ..
            },
            wir::Action::Call {
                name: nb, args: xb, ..
            },
        ) => na == nb && values_equivalent(a, b, xa, xb),
        _ => false,
    }
}

fn branches_equivalent(
    a: &wir::Program,
    b: &wir::Program,
    left: &[wir::IfBranch],
    right: &[wir::IfBranch],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right.iter()).all(|(la, rb)| {
            value_equivalent(a, b, la.condition, rb.condition)
                && actions_equivalent(a, b, &la.body, &rb.body)
        })
}

fn actions_equivalent(
    a: &wir::Program,
    b: &wir::Program,
    left: &[wir::ActionId],
    right: &[wir::ActionId],
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(la, rb)| action_equivalent(a, b, *la, *rb))
}

fn bodies_equivalent(
    a: &wir::Program,
    b: &wir::Program,
    left: &Option<Vec<wir::ActionId>>,
    right: &Option<Vec<wir::ActionId>>,
) -> bool {
    match (left, right) {
        (Some(la), Some(rb)) => actions_equivalent(a, b, la, rb),
        (None, None) => true,
        _ => false,
    }
}

fn value_equivalent(
    a: &wir::Program,
    b: &wir::Program,
    left: wir::ValueId,
    right: wir::ValueId,
) -> bool {
    let (Some(la), Some(rb)) = (a.values.get(left), b.values.get(right)) else {
        return false;
    };
    match (&la.value, &rb.value) {
        (wir::Value::Number(x), wir::Value::Number(y)) => x == y,
        (wir::Value::String(x), wir::Value::String(y)) => x == y,
        (wir::Value::Bool(x), wir::Value::Bool(y)) => x == y,
        (wir::Value::Null, wir::Value::Null) => true,
        (wir::Value::Array(xa), wir::Value::Array(xb)) => values_equivalent(a, b, xa, xb),
        (
            wir::Value::Vector {
                x: x1,
                y: y1,
                z: z1,
            },
            wir::Value::Vector {
                x: x2,
                y: y2,
                z: z2,
            },
        ) => {
            value_equivalent(a, b, *x1, *x2)
                && value_equivalent(a, b, *y1, *y2)
                && value_equivalent(a, b, *z1, *z2)
        }
        (
            wir::Value::Enum {
                value_type: t1,
                value: v1,
            },
            wir::Value::Enum {
                value_type: t2,
                value: v2,
            },
        ) => t1 == t2 && v1 == v2,
        (wir::Value::GlobalVariable(v1), wir::Value::GlobalVariable(v2)) => {
            name_eq(a.global_variables.get(*v1), b.global_variables.get(*v2))
        }
        (
            wir::Value::PlayerVariable {
                player: p1,
                variable: v1,
            },
            wir::Value::PlayerVariable {
                player: p2,
                variable: v2,
            },
        ) => {
            value_equivalent(a, b, *p1, *p2)
                && name_eq(a.player_variables.get(*v1), b.player_variables.get(*v2))
        }
        (wir::Value::EventPlayer, wir::Value::EventPlayer) => true,
        (wir::Value::Call { name: n1, args: x1 }, wir::Value::Call { name: n2, args: x2 }) => {
            n1 == n2 && values_equivalent(a, b, x1, x2)
        }
        _ => false,
    }
}

fn values_equivalent(
    a: &wir::Program,
    b: &wir::Program,
    left: &[wir::ValueId],
    right: &[wir::ValueId],
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(la, rb)| value_equivalent(a, b, *la, *rb))
}

fn name_eq<T: Named>(left: Option<&T>, right: Option<&T>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.name() == right.name(),
        (None, None) => true,
        _ => false,
    }
}

trait Named {
    fn name(&self) -> &str;
}

impl Named for wir::WorkshopVariable {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Named for wir::WorkshopSubroutine {
    fn name(&self) -> &str {
        &self.name
    }
}

fn sha256(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}
