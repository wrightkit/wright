//! Validated source-edit operations on the session (#434):
//! `validateEditTransaction` and `semanticRename` run on raw Workshop
//! input through `workshop-rs` reparse and identifier provenance; other
//! source languages stay at the provider boundary. `rename` is the
//! `wright rename` CLI workflow — a validated diff by default, an atomic
//! filesystem update with `--write`.

use std::collections::BTreeMap;

use super::CompilerSession;
use crate::diag::Diagnostic;
use crate::edit::{EditTransaction, EditValidation, RenameResult, RenameTarget, SemanticRename};
use crate::result::Envelope;
use crate::service::Address;

impl CompilerSession {
    /// `validateEditTransaction` (#434): apply `transaction` to the supplied
    /// current sources and, on raw Workshop input, reparse and validate the
    /// edited result through `workshop-rs`. When `sources` is absent (#472)
    /// the current text of the files the transaction names is read from
    /// disk; when present it stays the caller's precondition text. OPY and
    /// other source languages refuse with `edit-requires-provider` naming
    /// `providerValidateEdit`.
    pub fn validate_edit_transaction(
        &self,
        sources: Option<&BTreeMap<String, String>>,
        transaction: &EditTransaction,
    ) -> EditValidation {
        if let Err(diagnostic) = self.verify_fixed_config() {
            let diagnostics = vec![diagnostic];
            return EditValidation {
                ok: false,
                exit: crate::result::exit_code_from(&diagnostics),
                diagnostics,
                preview: None,
            };
        }
        crate::edit::validate_transaction(&self.config, &self.catalog, sources, transaction)
    }

    /// `semanticRename` (#434): resolve `target` against the loaded
    /// program's semantic index and rewrite every identifier occurrence
    /// through `workshop-rs` provenance — never a textual search. When
    /// `sources` is absent (#472) the loaded input's current text is read
    /// from disk; when present it stays the caller's precondition text.
    /// OPY and other source languages refuse with `edit-requires-provider`
    /// naming `providerSemanticRename`, whether or not a provider is
    /// configured.
    pub fn semantic_rename(
        &mut self,
        sources: Option<&BTreeMap<String, String>>,
        target: &RenameTarget,
    ) -> SemanticRename {
        let refuse = |diagnostics: Vec<Diagnostic>| SemanticRename {
            ok: false,
            transaction: None,
            diagnostics,
            preview: None,
        };
        if let Err(diagnostic) = self.verify_fixed_config() {
            return refuse(vec![diagnostic]);
        }
        match crate::edit::resolve_edit_input(&self.config) {
            Ok(resolved) => {
                if let Some(diagnostic) =
                    crate::edit::edit_kind_gate(resolved.kind, "providerSemanticRename")
                {
                    return refuse(vec![diagnostic]);
                }
            }
            Err(diagnostic) => return refuse(vec![diagnostic]),
        }
        match self.load() {
            Ok(loaded) => crate::edit::semantic_rename(&loaded, &self.catalog, sources, target),
            Err(diagnostic) => refuse(vec![diagnostic]),
        }
    }

    /// `wright rename <NAME> <TO> [INPUT]` (#434): the validated semantic
    /// rename of a Workshop variable or subroutine — the transaction and a
    /// source diff by default, an atomic filesystem update with `write`.
    /// A stale input or an invalid rename refuses with structured
    /// diagnostics and no partial write.
    pub fn rename(&mut self, name: &str, to: &str, write: bool) -> Envelope<RenameResult> {
        let mut result = RenameResult::default();
        if let Err(diagnostic) = self.verify_fixed_config() {
            self.diagnostics.push(diagnostic);
            return self.finish("rename", result);
        }
        match crate::edit::resolve_edit_input(&self.config) {
            Ok(resolved) => {
                if let Some(diagnostic) =
                    crate::edit::edit_kind_gate(resolved.kind, "providerSemanticRename")
                {
                    self.diagnostics.push(diagnostic);
                    return self.finish("rename", result);
                }
            }
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish("rename", result);
            }
        }
        let loaded = match self.load() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                return self.finish("rename", result);
            }
        };
        let sources = BTreeMap::from([(loaded.input.display.clone(), loaded.input.text.clone())]);
        let rename = crate::edit::semantic_rename(
            &loaded,
            &self.catalog,
            Some(&sources),
            &RenameTarget {
                symbol: Some(Address::Name(name.to_string())),
                source: None,
                line: None,
                col: None,
                to: to.to_string(),
            },
        );
        self.diagnostics.extend(rename.diagnostics);
        result.transaction = rename.transaction;
        result.preview = rename.preview;
        result.originals = sources;
        if !rename.ok {
            return self.finish("rename", result);
        }
        if write {
            match crate::edit::write_previews(
                result.preview.as_deref().unwrap_or_default(),
                result
                    .transaction
                    .as_ref()
                    .expect("a successful rename carries its transaction"),
            ) {
                Ok(written) => result.written = written,
                Err(diagnostic) => self.diagnostics.push(diagnostic),
            }
        }
        self.finish("rename", result)
    }
}
