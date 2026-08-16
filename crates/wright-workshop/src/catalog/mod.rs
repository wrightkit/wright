//! The canonical Workshop catalog.
//!
//! The catalog is Wright's locale-independent semantic identity layer between
//! textual Workshop spellings and WIR. Every builtin has a canonical `id` and
//! a [`Kind`]; localized aliases map to ids and back, so parser, emitter,
//! analyzer, and tooling never embed locale-specific strings as identity.
//!
//! The v0.2 catalog data (`data/catalog.json`) is authored from the
//! support-matrix evidence ([`docs/workshop/support-matrix.md`]) and covers
//! the corpus surface in `en-US`; additional locales are an explicit
//! data-pipeline change, not a code change.

use std::collections::HashMap;

use serde::Deserialize;

use wright_core::signatures::ExpectedDomain;

use crate::error::{CatalogError, Result};

/// The embedded v0.2 catalog data.
pub const CATALOG_DATA: &str = include_str!("data/catalog.json");

/// A normalized Workshop client locale, e.g. `en-US`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Locale(String);

impl Locale {
    /// Build a locale from a client spelling, normalized to lowercase.
    pub fn new(value: &str) -> Locale {
        Locale(value.trim().to_ascii_lowercase())
    }

    /// The normalized locale string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Locale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The kind of a catalog builtin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A structural keyword (If, End, Set Global Variable, …).
    Structural,
    /// An action function.
    Action,
    /// A value function.
    Value,
    /// An event.
    Event,
    /// An operator token (comparison operators).
    Operator,
    /// An enumerated value domain.
    Enum,
    /// A settings entry.
    Setting,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Structural => "structural",
            Kind::Action => "action",
            Kind::Value => "value",
            Kind::Event => "event",
            Kind::Operator => "operator",
            Kind::Enum => "enum",
            Kind::Setting => "setting",
        }
    }
}

/// One catalog builtin.
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub id: String,
    pub kind: Kind,
    /// Parameter names, when the catalog documents them.
    pub params: Vec<String>,
    /// The canonical enum domain expected at each parameter position, when
    /// the parameter takes an enumerated value (parallel to `params`).
    /// `None` for non-enum parameters and for undocumented parameters.
    pub param_domains: Vec<Option<String>>,
    /// Wright-owned default value per parameter position (parallel to
    /// `params`), resolved when a call omits the argument (#119). See the
    /// catalog data provenance for the value syntax and evidence.
    pub param_defaults: Vec<Option<String>>,
    aliases: HashMap<Locale, String>,
}

impl CatalogEntry {
    /// The localized spelling of this builtin in `locale`.
    pub fn spelling(&self, locale: &Locale) -> Option<&str> {
        self.aliases.get(locale).map(String::as_str)
    }
}

/// One enum member within a domain.
#[derive(Debug, Clone)]
pub struct EnumMember {
    pub member: String,
    aliases: HashMap<Locale, String>,
}

impl EnumMember {
    /// The localized spelling of this member in `locale`.
    pub fn spelling(&self, locale: &Locale) -> Option<&str> {
        self.aliases.get(locale).map(String::as_str)
    }
}

/// One enum value domain (e.g. `Color`, `Beam`).
#[derive(Debug, Clone)]
pub struct EnumDomain {
    pub domain: String,
    pub members: Vec<EnumMember>,
}

/// Target-format metadata recorded in the catalog.
#[derive(Debug, Clone, Deserialize)]
pub struct TargetMeta {
    pub game: String,
    pub format: String,
    pub surface: String,
}

/// Provenance of the catalog data.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub generator: String,
    pub generator_version: String,
    pub source: String,
    pub license: String,
    pub reviewed: bool,
}

/// The validated canonical Workshop catalog.
#[derive(Debug, Clone)]
pub struct Catalog {
    pub schema_version: u32,
    pub locales: Vec<Locale>,
    pub target: TargetMeta,
    pub provenance: Provenance,
    entries: Vec<CatalogEntry>,
    enums: Vec<EnumDomain>,
    by_id: HashMap<(Kind, String), usize>,
    alias_to_entry: HashMap<(Kind, Locale, String), usize>,
    enum_by_domain: HashMap<String, usize>,
    enum_alias_to_member: HashMap<(String, Locale, String), (usize, usize)>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogFile {
    schema_version: u32,
    locales: Vec<String>,
    target: TargetMeta,
    provenance: Provenance,
    #[serde(default)]
    structural: Vec<EntryFile>,
    #[serde(default)]
    actions: Vec<EntryFile>,
    #[serde(default)]
    values: Vec<EntryFile>,
    #[serde(default)]
    events: Vec<EntryFile>,
    #[serde(default)]
    operators: Vec<EntryFile>,
    #[serde(default)]
    settings: Vec<EntryFile>,
    #[serde(default)]
    enums: Vec<EnumFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryFile {
    id: String,
    aliases: HashMap<String, String>,
    #[serde(default)]
    params: Vec<String>,
    /// Canonical enum domain per parameter position (parallel to `params`);
    /// empty when no parameter domains are documented.
    #[serde(default)]
    param_domains: Vec<Option<String>>,
    /// Wright-owned default value per parameter position (parallel to
    /// `params`), resolved by the OSTW semantic phase when a call omits the
    /// argument (#119). `None` means no default is declared. Default value
    /// syntax: `null`, a numeric literal, `Domain.MEMBER` (builtin enum
    /// member), or a catalog value id resolved as a zero-argument call.
    /// Every default is pinned-reference probe evidence (P6/P6b), never
    /// copied from upstream game data.
    #[serde(default)]
    param_defaults: Vec<Option<String>>,
}

#[derive(Deserialize)]
struct EnumFile {
    domain: String,
    members: Vec<MemberFile>,
}

#[derive(Deserialize)]
struct MemberFile {
    id: String,
    aliases: HashMap<String, String>,
}

impl Catalog {
    /// Parse and validate catalog data.
    pub fn load(json: &str) -> Result<Catalog> {
        let file: CatalogFile = serde_json::from_str(json)
            .map_err(|error| CatalogError::malformed(format!("catalog data: {error}")))?;
        if file.schema_version != 1 {
            return Err(CatalogError::malformed(format!(
                "unsupported catalog schemaVersion {}",
                file.schema_version
            )));
        }
        let locales: Vec<Locale> = file.locales.iter().map(|s| Locale::new(s)).collect();
        if locales.is_empty() {
            return Err(CatalogError::malformed(
                "catalog declares no locales".to_string(),
            ));
        }

        let mut catalog = Catalog {
            schema_version: file.schema_version,
            locales,
            target: file.target,
            provenance: file.provenance,
            entries: Vec::new(),
            enums: Vec::new(),
            by_id: HashMap::new(),
            alias_to_entry: HashMap::new(),
            enum_by_domain: HashMap::new(),
            enum_alias_to_member: HashMap::new(),
        };

        for (kind, items) in [
            (Kind::Structural, file.structural),
            (Kind::Action, file.actions),
            (Kind::Value, file.values),
            (Kind::Event, file.events),
            (Kind::Operator, file.operators),
            (Kind::Setting, file.settings),
        ] {
            for item in items {
                catalog.insert_entry(kind, item)?;
            }
        }
        for domain in file.enums {
            catalog.insert_enum(domain)?;
        }
        catalog.validate_param_domains()?;
        Ok(catalog)
    }

    /// The built-in v0.2 catalog.
    pub fn builtin() -> Result<Catalog> {
        Self::load(CATALOG_DATA)
    }

    /// The declared locales, normalized.
    pub fn locales(&self) -> &[Locale] {
        &self.locales
    }

    /// Whether a locale is declared by the catalog.
    pub fn supports(&self, locale: &Locale) -> bool {
        self.locales.contains(locale)
    }

    /// The builtin with the given canonical id and kind.
    pub fn entry(&self, kind: Kind, id: &str) -> Option<&CatalogEntry> {
        self.by_id
            .get(&(kind, id.to_string()))
            .map(|i| &self.entries[*i])
    }

    /// Resolve a localized spelling to its canonical builtin.
    pub fn resolve(&self, kind: Kind, locale: &Locale, spelling: &str) -> Option<&CatalogEntry> {
        self.alias_to_entry
            .get(&(kind, locale.clone(), spelling.to_string()))
            .map(|i| &self.entries[*i])
    }

    /// The localized spelling of a canonical builtin id.
    pub fn spelling(&self, kind: Kind, locale: &Locale, id: &str) -> Option<&str> {
        self.entry(kind, id)?.spelling(locale)
    }

    /// Every entry of a kind, in catalog order.
    pub fn entries_of(&self, kind: Kind) -> impl Iterator<Item = &CatalogEntry> {
        self.entries.iter().filter(move |entry| entry.kind == kind)
    }

    /// The total number of builtin entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// The number of enum domains.
    pub fn enum_domains_count(&self) -> usize {
        self.enums.len()
    }

    /// The enum domain with the given name.
    pub fn enum_domain(&self, domain: &str) -> Option<&EnumDomain> {
        self.enum_by_domain.get(domain).map(|i| &self.enums[*i])
    }

    /// Every enum domain, in catalog order.
    pub fn enum_domains(&self) -> impl Iterator<Item = &EnumDomain> {
        self.enums.iter()
    }

    /// Resolve a localized enum member spelling to `(domain, canonical member)`.
    pub fn resolve_enum_member(
        &self,
        domain: &str,
        locale: &Locale,
        spelling: &str,
    ) -> Option<(String, String)> {
        let (domain_index, member_index) = self.enum_alias_to_member.get(&(
            domain.to_string(),
            locale.clone(),
            spelling.to_string(),
        ))?;
        Some((
            domain.to_string(),
            self.enums[*domain_index].members[*member_index]
                .member
                .clone(),
        ))
    }

    /// The localized spelling of a canonical enum member.
    pub fn enum_spelling(&self, domain: &str, locale: &Locale, member: &str) -> Option<&str> {
        let domain_index = self.enum_by_domain.get(domain)?;
        let domain = &self.enums[*domain_index];
        domain
            .members
            .iter()
            .find(|candidate| candidate.member == member)?
            .spelling(locale)
    }

    /// Every `(domain, canonical member)` match for a bare (domain-less)
    /// localized member spelling. Returns all matches so callers can report
    /// ambiguity; a well-formed catalog has at most one meaningful match for
    /// a given spelling.
    pub fn bare_member_matches(&self, locale: &Locale, spelling: &str) -> Vec<(String, String)> {
        let mut matches = Vec::new();
        for domain in &self.enums {
            for member in &domain.members {
                if member.spelling(locale) == Some(spelling) {
                    matches.push((domain.domain.clone(), member.member.clone()));
                }
            }
        }
        matches
    }

    fn insert_entry(&mut self, kind: Kind, item: EntryFile) -> Result<()> {
        let index = self.entries.len();
        let mut aliases = HashMap::new();
        for (locale_str, spelling) in &item.aliases {
            let locale = Locale::new(locale_str);
            if !self.locales.contains(&locale) {
                return Err(CatalogError::validation(format!(
                    "entry '{}' declares alias for undeclared locale '{}'",
                    item.id, locale
                )));
            }
            let key = (kind, locale.clone(), spelling.clone());
            if self.alias_to_entry.contains_key(&key) {
                return Err(CatalogError::validation(format!(
                    "duplicate {} alias '{spelling}' for locale '{}'",
                    kind.as_str(),
                    locale
                )));
            }
            aliases.insert(locale, spelling.clone());
            self.alias_to_entry.insert(key, index);
        }
        let id_key = (kind, item.id.clone());
        if self.by_id.contains_key(&id_key) {
            return Err(CatalogError::validation(format!(
                "duplicate {} id '{}'",
                kind.as_str(),
                item.id
            )));
        }
        // Every declared locale needs an alias.
        for locale in &self.locales {
            if !aliases.contains_key(locale) {
                return Err(CatalogError::validation(format!(
                    "{} '{}' is missing a '{}' alias",
                    kind.as_str(),
                    item.id,
                    locale
                )));
            }
        }
        self.by_id.insert(id_key, index);
        self.entries.push(CatalogEntry {
            id: item.id,
            kind,
            params: item.params,
            param_domains: item.param_domains,
            param_defaults: item.param_defaults,
            aliases,
        });
        Ok(())
    }

    /// Every declared `paramDomains` domain must name a declared enum domain.
    fn validate_param_domains(&self) -> Result<()> {
        for entry in &self.entries {
            if entry.param_domains.len() > entry.params.len() {
                return Err(CatalogError::validation(format!(
                    "{} '{}' declares more param domains than params",
                    entry.kind.as_str(),
                    entry.id
                )));
            }
            if entry.param_defaults.len() > entry.params.len() {
                return Err(CatalogError::validation(format!(
                    "{} '{}' declares more param defaults than params",
                    entry.kind.as_str(),
                    entry.id
                )));
            }
            for domain in entry.param_domains.iter().flatten() {
                if !self.enum_by_domain.contains_key(domain) {
                    return Err(CatalogError::validation(format!(
                        "{} '{}' declares undeclared enum domain '{domain}'",
                        entry.kind.as_str(),
                        entry.id
                    )));
                }
            }
        }
        Ok(())
    }

    fn insert_enum(&mut self, domain: EnumFile) -> Result<()> {
        let domain_index = self.enums.len();
        if self.enum_by_domain.contains_key(&domain.domain) {
            return Err(CatalogError::validation(format!(
                "duplicate enum domain '{}'",
                domain.domain
            )));
        }
        let mut members = Vec::new();
        for (member_index, member) in domain.members.into_iter().enumerate() {
            let mut aliases = HashMap::new();
            for (locale_str, spelling) in &member.aliases {
                let locale = Locale::new(locale_str);
                if !self.locales.contains(&locale) {
                    return Err(CatalogError::validation(format!(
                        "enum {}::{} declares alias for undeclared locale '{}'",
                        domain.domain, member.id, locale
                    )));
                }
                let key = (domain.domain.clone(), locale.clone(), spelling.clone());
                if self.enum_alias_to_member.contains_key(&key) {
                    return Err(CatalogError::validation(format!(
                        "duplicate enum alias '{spelling}' in '{}' for locale '{}'",
                        domain.domain, locale
                    )));
                }
                aliases.insert(locale, spelling.clone());
                self.enum_alias_to_member
                    .insert(key, (domain_index, member_index));
            }
            for locale in &self.locales {
                if !aliases.contains_key(locale) {
                    return Err(CatalogError::validation(format!(
                        "enum {}::{} is missing a '{}' alias",
                        domain.domain, member.id, locale
                    )));
                }
            }
            members.push(EnumMember {
                member: member.id,
                aliases,
            });
        }
        self.enum_by_domain
            .insert(domain.domain.clone(), domain_index);
        self.enums.push(EnumDomain {
            domain: domain.domain,
            members,
        });
        Ok(())
    }
}

/// The catalog is the canonical source of expected enum domains for the
/// Workshop surface it documents: `expected_domain(catalog_id, arg_index)`
/// answers the domain declared for that parameter position (e.g. `createHudText`
/// argument 9 is `HudReeval`), so the Workshop parser can resolve bare enum
/// members that are ambiguous across domains (e.g. `Visible To and String`,
/// #118). Positions without a documented domain answer `None`.
impl ExpectedDomain for Catalog {
    fn expected_domain(&self, catalog_id: &str, arg_index: usize) -> Option<&str> {
        for kind in [Kind::Action, Kind::Value] {
            if let Some(entry) = self.entry(kind, catalog_id) {
                if let Some(domain) = entry
                    .param_domains
                    .get(arg_index)
                    .and_then(Option::as_deref)
                {
                    return Some(domain);
                }
            }
        }
        None
    }
}

/// Canonicalize catalog data: parse, validate, and re-serialize deterministically
/// (object keys sorted, stable formatting). Re-running on the same input
/// produces byte-identical output, so the data pipeline is reproducible.
pub fn canonicalize(json: &str) -> Result<String> {
    // Validate the semantic content first.
    Catalog::load(json)?;
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| CatalogError::malformed(format!("catalog data: {error}")))?;
    serde_json::to_string_pretty(&value)
        .map(|mut out| {
            out.push('\n');
            out
        })
        .map_err(|error| CatalogError::malformed(format!("cannot serialize catalog: {error}")))
}
