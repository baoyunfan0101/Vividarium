use std::collections::HashSet;

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::{CoreError, CoreResult, Database};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaxonRank {
    Kingdom,
    Order,
    Family,
    Genus,
    Species,
}

impl TaxonRank {
    const ALL: [Self; 5] = [
        Self::Kingdom,
        Self::Order,
        Self::Family,
        Self::Genus,
        Self::Species,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::Kingdom => "kingdom",
            Self::Order => "order",
            Self::Family => "family",
            Self::Genus => "genus",
            Self::Species => "species",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Kingdom => 0,
            Self::Order => 1,
            Self::Family => 2,
            Self::Genus => 3,
            Self::Species => 4,
        }
    }

    fn parent(self) -> Option<Self> {
        match self {
            Self::Kingdom => None,
            Self::Order => Some(Self::Kingdom),
            Self::Family => Some(Self::Order),
            Self::Genus => Some(Self::Family),
            Self::Species => Some(Self::Genus),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonNameInput {
    pub name: String,
    pub is_accepted: Option<bool>,
    pub authority_year: Option<String>,
    pub category: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonInputRow {
    pub selected_taxon_id: Option<i64>,
    pub kingdom: Option<String>,
    pub order: Option<String>,
    pub family: Option<String>,
    pub genus: Option<String>,
    pub species: Option<String>,
    pub geological_range: Option<String>,
    pub scientific: Option<TaxonNameInput>,
    pub english: Option<TaxonNameInput>,
    pub chinese: Option<TaxonNameInput>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonUpdateOptions {
    pub allow_new_names: bool,
    pub allow_new_taxa: bool,
    pub allow_overwrite: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonPath {
    pub kingdom: Option<String>,
    pub order: Option<String>,
    pub family: Option<String>,
    pub genus: Option<String>,
    pub species: Option<String>,
}

impl TaxonPath {
    fn set(&mut self, rank: TaxonRank, value: String) {
        match rank {
            TaxonRank::Kingdom => self.kingdom = Some(value),
            TaxonRank::Order => self.order = Some(value),
            TaxonRank::Family => self.family = Some(value),
            TaxonRank::Genus => self.genus = Some(value),
            TaxonRank::Species => self.species = Some(value),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonCandidate {
    pub taxon_id: i64,
    pub rank: TaxonRank,
    pub scientific_name: String,
    pub path: TaxonPath,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaxonRowStatus {
    Ready,
    NoChange,
    NotFound,
    Ambiguous,
    Conflict,
    Invalid,
}

impl TaxonRowStatus {
    fn blocks_commit(self) -> bool {
        matches!(
            self,
            Self::NotFound | Self::Ambiguous | Self::Conflict | Self::Invalid
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaxonChangeKind {
    CreateTaxon,
    AppendName,
    Overwrite,
    ChangeAcceptedName,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonChange {
    pub kind: TaxonChangeKind,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonRowOutcome {
    pub row_number: usize,
    pub status: TaxonRowStatus,
    pub message: String,
    pub target: Option<TaxonCandidate>,
    pub candidates: Vec<TaxonCandidate>,
    pub changes: Vec<TaxonChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonBatchResult {
    pub committed: bool,
    pub rows: Vec<TaxonRowOutcome>,
}

pub fn preview_taxon_rows(
    database: &Database,
    rows: &[TaxonInputRow],
    options: TaxonUpdateOptions,
) -> CoreResult<TaxonBatchResult> {
    process_taxon_rows(database, rows, options, false)
}

pub fn apply_taxon_rows(
    database: &Database,
    rows: &[TaxonInputRow],
    options: TaxonUpdateOptions,
) -> CoreResult<TaxonBatchResult> {
    process_taxon_rows(database, rows, options, true)
}

fn process_taxon_rows(
    database: &Database,
    rows: &[TaxonInputRow],
    options: TaxonUpdateOptions,
    commit_requested: bool,
) -> CoreResult<TaxonBatchResult> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut outcomes = Vec::with_capacity(rows.len());
    let mut blocked = false;
    for (index, row) in rows.iter().enumerate() {
        let outcome = match prepare_row(&transaction, row, options) {
            Ok(plan) => execute_plan(&transaction, index + 1, plan)?,
            Err(issue) => TaxonRowOutcome {
                row_number: index + 1,
                status: issue.status,
                message: issue.message,
                target: None,
                candidates: issue.candidates,
                changes: Vec::new(),
            },
        };
        blocked |= outcome.status.blocks_commit();
        outcomes.push(outcome);
    }
    let committed = commit_requested && !blocked;
    if committed {
        transaction.commit()?;
    } else {
        transaction.rollback()?;
    }
    Ok(TaxonBatchResult {
        committed,
        rows: outcomes,
    })
}

#[derive(Debug)]
struct RowIssue {
    status: TaxonRowStatus,
    message: String,
    candidates: Vec<TaxonCandidate>,
}

impl RowIssue {
    fn new(status: TaxonRowStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            candidates: Vec::new(),
        }
    }

    fn ambiguous(message: impl Into<String>, candidates: Vec<TaxonCandidate>) -> Self {
        Self {
            status: TaxonRowStatus::Ambiguous,
            message: message.into(),
            candidates,
        }
    }
}

#[derive(Debug, Clone)]
struct NormalizedPath {
    values: [Option<String>; 5],
}

impl NormalizedPath {
    fn from_row(row: &TaxonInputRow) -> Self {
        Self {
            values: [
                normalize(row.kingdom.as_deref()),
                normalize(row.order.as_deref()),
                normalize(row.family.as_deref()),
                normalize(row.genus.as_deref()),
                normalize(row.species.as_deref()),
            ],
        }
    }

    fn get(&self, rank: TaxonRank) -> Option<&str> {
        self.values[rank.index()].as_deref()
    }

    fn set(&mut self, rank: TaxonRank, value: String) {
        self.values[rank.index()] = Some(value);
    }

    fn deepest(&self) -> Option<(TaxonRank, &str)> {
        TaxonRank::ALL
            .into_iter()
            .rev()
            .find_map(|rank| self.get(rank).map(|name| (rank, name)))
    }
}

#[derive(Debug)]
struct RowPlan {
    target: PlannedTarget,
    geological_range: Option<FieldUpdate>,
    names: Vec<NamePlan>,
    changes: Vec<TaxonChange>,
}

#[derive(Debug)]
enum PlannedTarget {
    Existing(i64),
    New {
        rank: TaxonRank,
        parent_taxon_id: Option<i64>,
        geological_range: Option<String>,
    },
}

#[derive(Debug)]
struct FieldUpdate {
    value: String,
}

#[derive(Debug, Clone, Copy)]
enum NameKind {
    Scientific,
    English,
    Chinese,
}

impl NameKind {
    fn table(self) -> &'static str {
        match self {
            Self::Scientific => "scientific",
            Self::English => "english",
            Self::Chinese => "chinese",
        }
    }

    fn column(self) -> &'static str {
        match self {
            Self::Scientific => "scientific_name",
            Self::English => "english_name",
            Self::Chinese => "chinese_name",
        }
    }
}

#[derive(Debug)]
struct NamePlan {
    kind: NameKind,
    name: String,
    insert: Option<NameRecord>,
    updates: Vec<NameFieldUpdate>,
    demote_accepted: Option<String>,
    promote: bool,
}

#[derive(Debug, Clone)]
struct NameRecord {
    is_accepted: bool,
    authority_year: Option<String>,
    category: Option<String>,
    source: Option<String>,
}

#[derive(Debug)]
struct NameFieldUpdate {
    field: NameField,
    value: String,
}

#[derive(Debug, Clone, Copy)]
enum NameField {
    AuthorityYear,
    Category,
    Source,
}

impl NameField {
    fn as_str(self) -> &'static str {
        match self {
            Self::AuthorityYear => "authority_year",
            Self::Category => "category",
            Self::Source => "source",
        }
    }
}

fn prepare_row(
    transaction: &Transaction<'_>,
    row: &TaxonInputRow,
    options: TaxonUpdateOptions,
) -> Result<RowPlan, RowIssue> {
    let mut path = NormalizedPath::from_row(row);
    let (target_rank, target_name) = path.deepest().ok_or_else(|| {
        RowIssue::new(
            TaxonRowStatus::Invalid,
            "at least one rank scientific name is required",
        )
    })?;
    let target_name = target_name.to_string();
    validate_required_parent(target_rank, &target_name, &mut path)?;
    let search =
        find_candidates(transaction, target_rank, &target_name, &path).map_err(core_issue)?;
    let had_name_match = search.had_name_match;
    let mut candidates = search.candidates;
    if let Some(selected) = row.selected_taxon_id {
        if let Some(position) = candidates
            .iter()
            .position(|candidate| candidate.taxon_id == selected)
        {
            candidates = vec![candidates.swap_remove(position)];
        } else if !candidates.is_empty() {
            return Err(RowIssue::new(
                TaxonRowStatus::Invalid,
                "selected_taxon_id is not one of the matching candidates",
            ));
        }
    }
    match candidates.len() {
        0 if row.selected_taxon_id.is_some() => Err(RowIssue::new(
            TaxonRowStatus::Invalid,
            "selected_taxon_id does not match the row locator",
        )),
        0 if had_name_match => Err(RowIssue::new(
            TaxonRowStatus::Conflict,
            format!(
                "{} '{}' exists, but its lineage does not match the coarse rank filters",
                target_rank.as_str(),
                target_name
            ),
        )),
        0 if !options.allow_new_taxa => Err(RowIssue::new(
            TaxonRowStatus::NotFound,
            format!("{} '{}' was not found", target_rank.as_str(), target_name),
        )),
        0 => prepare_new_taxon(transaction, row, options, path, target_rank, target_name),
        1 => prepare_existing_taxon(transaction, row, options, candidates[0].taxon_id),
        _ => Err(RowIssue::ambiguous(
            format!(
                "{} '{}' matched multiple taxa",
                target_rank.as_str(),
                target_name
            ),
            candidates,
        )),
    }
}

fn validate_required_parent(
    target_rank: TaxonRank,
    target_name: &str,
    path: &mut NormalizedPath,
) -> Result<(), RowIssue> {
    let Some(parent_rank) = target_rank.parent() else {
        return Ok(());
    };
    if path.get(parent_rank).is_some() {
        return Ok(());
    }
    if target_rank != TaxonRank::Species {
        return Err(RowIssue::new(
            TaxonRowStatus::Invalid,
            format!(
                "{} requires its immediate parent {} scientific name",
                target_rank.as_str(),
                parent_rank.as_str()
            ),
        ));
    }
    let mut words = target_name.split_whitespace();
    let genus = words.next().unwrap_or_default();
    if genus.is_empty() || words.next().is_none() {
        return Err(RowIssue::new(
            TaxonRowStatus::Invalid,
            "species without an explicit genus must use a binomial scientific name",
        ));
    }
    path.set(TaxonRank::Genus, genus.to_string());
    Ok(())
}

fn prepare_new_taxon(
    transaction: &Transaction<'_>,
    row: &TaxonInputRow,
    _options: TaxonUpdateOptions,
    path: NormalizedPath,
    target_rank: TaxonRank,
    target_name: String,
) -> Result<RowPlan, RowIssue> {
    let parent_taxon_id = if let Some(parent_rank) = target_rank.parent() {
        let parent_name = path.get(parent_rank).ok_or_else(|| {
            RowIssue::new(
                TaxonRowStatus::Invalid,
                "the immediate parent scientific name is required",
            )
        })?;
        let parent_search =
            find_candidates(transaction, parent_rank, parent_name, &path).map_err(core_issue)?;
        let parents = parent_search.candidates;
        match parents.len() {
            0 if parent_search.had_name_match => {
                return Err(RowIssue::new(
                    TaxonRowStatus::Conflict,
                    format!(
                        "parent {} '{}' exists, but its lineage does not match the coarse rank filters",
                        parent_rank.as_str(),
                        parent_name
                    ),
                ));
            }
            0 => {
                return Err(RowIssue::new(
                    TaxonRowStatus::NotFound,
                    format!(
                        "parent {} '{}' was not found",
                        parent_rank.as_str(),
                        parent_name
                    ),
                ));
            }
            1 => Some(parents[0].taxon_id),
            _ => {
                return Err(RowIssue::ambiguous(
                    format!(
                        "parent {} '{}' matched multiple taxa",
                        parent_rank.as_str(),
                        parent_name
                    ),
                    parents,
                ));
            }
        }
    } else {
        None
    };
    let geological_range = normalize(row.geological_range.as_deref());
    let mut changes = vec![TaxonChange {
        kind: TaxonChangeKind::CreateTaxon,
        field: "taxa".into(),
        old_value: None,
        new_value: Some(format!("{}:{target_name}", target_rank.as_str())),
    }];
    let mut names = prepare_new_taxon_names(row, target_name, &mut changes)?;
    for (kind, input) in [
        (NameKind::English, row.english.as_ref()),
        (NameKind::Chinese, row.chinese.as_ref()),
    ] {
        if let Some(input) = input {
            names.push(new_first_name(kind, input, &mut changes)?);
        }
    }
    Ok(RowPlan {
        target: PlannedTarget::New {
            rank: target_rank,
            parent_taxon_id,
            geological_range,
        },
        geological_range: None,
        names,
        changes,
    })
}

fn prepare_new_taxon_names(
    row: &TaxonInputRow,
    locator_name: String,
    changes: &mut Vec<TaxonChange>,
) -> Result<Vec<NamePlan>, RowIssue> {
    let mut names = Vec::new();
    match row.scientific.as_ref() {
        Some(input) if normalize(Some(&input.name)).as_deref() == Some(locator_name.as_str()) => {
            if input.is_accepted == Some(false) {
                return Err(RowIssue::new(
                    TaxonRowStatus::Conflict,
                    "a new taxon's only scientific name must be accepted",
                ));
            }
            names.push(insert_name_plan(
                NameKind::Scientific,
                locator_name,
                input,
                true,
                changes,
            )?);
        }
        Some(input) => {
            let input_name = required_name(input)?;
            let input_accepted = input.is_accepted == Some(true);
            names.push(insert_name_plan(
                NameKind::Scientific,
                locator_name,
                &TaxonNameInput::default(),
                !input_accepted,
                changes,
            )?);
            names.push(insert_name_plan(
                NameKind::Scientific,
                input_name,
                input,
                input_accepted,
                changes,
            )?);
        }
        None => names.push(insert_name_plan(
            NameKind::Scientific,
            locator_name,
            &TaxonNameInput::default(),
            true,
            changes,
        )?),
    }
    Ok(names)
}

fn new_first_name(
    kind: NameKind,
    input: &TaxonNameInput,
    changes: &mut Vec<TaxonChange>,
) -> Result<NamePlan, RowIssue> {
    if input.is_accepted == Some(false) {
        return Err(RowIssue::new(
            TaxonRowStatus::Conflict,
            format!("a new taxon's only {} name must be accepted", kind.table()),
        ));
    }
    insert_name_plan(kind, required_name(input)?, input, true, changes)
}

fn insert_name_plan(
    kind: NameKind,
    name: String,
    input: &TaxonNameInput,
    is_accepted: bool,
    changes: &mut Vec<TaxonChange>,
) -> Result<NamePlan, RowIssue> {
    if name.trim().is_empty() {
        return Err(RowIssue::new(
            TaxonRowStatus::Invalid,
            format!("{} name cannot be empty", kind.table()),
        ));
    }
    changes.push(TaxonChange {
        kind: TaxonChangeKind::AppendName,
        field: format!("{}.{}", kind.table(), kind.column()),
        old_value: None,
        new_value: Some(name.clone()),
    });
    Ok(NamePlan {
        kind,
        name,
        insert: Some(NameRecord {
            is_accepted,
            authority_year: normalize(input.authority_year.as_deref()),
            category: normalize(input.category.as_deref()),
            source: normalize(input.source.as_deref()),
        }),
        updates: Vec::new(),
        demote_accepted: None,
        promote: false,
    })
}

fn prepare_existing_taxon(
    transaction: &Transaction<'_>,
    row: &TaxonInputRow,
    options: TaxonUpdateOptions,
    taxon_id: i64,
) -> Result<RowPlan, RowIssue> {
    let mut changes = Vec::new();
    let geological_range = plan_geological_range(
        transaction,
        taxon_id,
        row.geological_range.as_deref(),
        options,
        &mut changes,
    )?;
    let mut names = Vec::new();
    for (kind, input) in [
        (NameKind::Scientific, row.scientific.as_ref()),
        (NameKind::English, row.english.as_ref()),
        (NameKind::Chinese, row.chinese.as_ref()),
    ] {
        if let Some(input) = input {
            names.push(plan_existing_name(
                transaction,
                taxon_id,
                kind,
                input,
                options,
                &mut changes,
            )?);
        }
    }
    Ok(RowPlan {
        target: PlannedTarget::Existing(taxon_id),
        geological_range,
        names,
        changes,
    })
}

fn plan_geological_range(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    input: Option<&str>,
    options: TaxonUpdateOptions,
    changes: &mut Vec<TaxonChange>,
) -> Result<Option<FieldUpdate>, RowIssue> {
    let Some(value) = normalize(input) else {
        return Ok(None);
    };
    let existing: Option<String> = transaction
        .query_row(
            "SELECT geological_range FROM taxa WHERE taxon_id = ?",
            [taxon_id],
            |row| row.get(0),
        )
        .map_err(database_issue)?;
    if existing.as_deref() == Some(&value) {
        return Ok(None);
    }
    if !options.allow_overwrite {
        return Err(RowIssue::new(
            TaxonRowStatus::Conflict,
            "geological_range differs and overwrite is not allowed",
        ));
    }
    changes.push(TaxonChange {
        kind: TaxonChangeKind::Overwrite,
        field: "taxa.geological_range".into(),
        old_value: existing,
        new_value: Some(value.clone()),
    });
    Ok(Some(FieldUpdate { value }))
}

fn plan_existing_name(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: NameKind,
    input: &TaxonNameInput,
    options: TaxonUpdateOptions,
    changes: &mut Vec<TaxonChange>,
) -> Result<NamePlan, RowIssue> {
    let name = required_name(input)?;
    let sql = format!(
        "SELECT is_accepted, authority_year, category, source FROM {} WHERE taxon_id = ? AND {} = ?",
        kind.table(),
        kind.column()
    );
    let existing = transaction
        .query_row(&sql, params![taxon_id, name], |row| {
            Ok(NameRecord {
                is_accepted: row.get::<_, i64>(0)? != 0,
                authority_year: row.get(1)?,
                category: row.get(2)?,
                source: row.get(3)?,
            })
        })
        .optional()
        .map_err(database_issue)?;
    let accepted_name = accepted_name(transaction, taxon_id, kind).map_err(database_issue)?;
    let total_names = count_names(transaction, taxon_id, kind).map_err(database_issue)?;
    let Some(existing) = existing else {
        if !options.allow_new_names {
            return Err(RowIssue::new(
                TaxonRowStatus::Conflict,
                format!("new {} names are not allowed", kind.table()),
            ));
        }
        let is_accepted = input.is_accepted.unwrap_or(total_names == 0);
        if !is_accepted && total_names == 0 {
            return Err(RowIssue::new(
                TaxonRowStatus::Conflict,
                format!("the first {} name must be accepted", kind.table()),
            ));
        }
        let demote_accepted = if is_accepted {
            match accepted_name {
                Some(value) if options.allow_overwrite => Some(value),
                Some(_) => {
                    return Err(RowIssue::new(
                        TaxonRowStatus::Conflict,
                        format!(
                            "{} already has an accepted name and overwrite is not allowed",
                            kind.table()
                        ),
                    ));
                }
                None => None,
            }
        } else {
            None
        };
        changes.push(TaxonChange {
            kind: TaxonChangeKind::AppendName,
            field: format!("{}.{}", kind.table(), kind.column()),
            old_value: None,
            new_value: Some(name.clone()),
        });
        if let Some(old) = demote_accepted.as_ref() {
            changes.push(accepted_change(kind, Some(old.clone()), name.clone()));
        }
        return Ok(NamePlan {
            kind,
            name,
            insert: Some(NameRecord {
                is_accepted,
                authority_year: normalize(input.authority_year.as_deref()),
                category: normalize(input.category.as_deref()),
                source: normalize(input.source.as_deref()),
            }),
            updates: Vec::new(),
            demote_accepted,
            promote: false,
        });
    };

    let mut plan = NamePlan {
        kind,
        name: name.clone(),
        insert: None,
        updates: Vec::new(),
        demote_accepted: None,
        promote: false,
    };
    for (field, old, new) in [
        (
            NameField::AuthorityYear,
            existing.authority_year,
            normalize(input.authority_year.as_deref()),
        ),
        (
            NameField::Category,
            existing.category,
            normalize(input.category.as_deref()),
        ),
        (
            NameField::Source,
            existing.source,
            normalize(input.source.as_deref()),
        ),
    ] {
        let Some(new) = new else {
            continue;
        };
        if old.as_deref() == Some(&new) {
            continue;
        }
        if !options.allow_overwrite {
            return Err(RowIssue::new(
                TaxonRowStatus::Conflict,
                format!(
                    "{}.{} differs and overwrite is not allowed",
                    kind.table(),
                    field.as_str()
                ),
            ));
        }
        changes.push(TaxonChange {
            kind: TaxonChangeKind::Overwrite,
            field: format!("{}.{}", kind.table(), field.as_str()),
            old_value: old,
            new_value: Some(new.clone()),
        });
        plan.updates.push(NameFieldUpdate { field, value: new });
    }
    match input.is_accepted {
        None => {}
        Some(value) if value == existing.is_accepted => {}
        Some(false) => {
            return Err(RowIssue::new(
                TaxonRowStatus::Conflict,
                "the accepted name cannot be demoted without selecting a replacement",
            ));
        }
        Some(true) if !options.allow_overwrite => {
            return Err(RowIssue::new(
                TaxonRowStatus::Conflict,
                format!(
                    "{}.is_accepted differs and overwrite is not allowed",
                    kind.table()
                ),
            ));
        }
        Some(true) => {
            plan.demote_accepted = accepted_name.filter(|accepted| accepted != &name);
            plan.promote = true;
            changes.push(accepted_change(kind, plan.demote_accepted.clone(), name));
        }
    }
    Ok(plan)
}

fn accepted_change(kind: NameKind, old: Option<String>, new: String) -> TaxonChange {
    TaxonChange {
        kind: TaxonChangeKind::ChangeAcceptedName,
        field: format!("{}.is_accepted", kind.table()),
        old_value: old,
        new_value: Some(new),
    }
}

fn execute_plan(
    transaction: &Transaction<'_>,
    row_number: usize,
    plan: RowPlan,
) -> CoreResult<TaxonRowOutcome> {
    let taxon_id = match plan.target {
        PlannedTarget::Existing(taxon_id) => {
            if let Some(update) = plan.geological_range {
                transaction.execute(
                    "UPDATE taxa SET geological_range = ? WHERE taxon_id = ?",
                    params![update.value, taxon_id],
                )?;
            }
            taxon_id
        }
        PlannedTarget::New {
            rank,
            parent_taxon_id,
            geological_range,
        } => {
            transaction.execute(
                "INSERT INTO taxa (parent_taxon_id, rank, geological_range) VALUES (?, ?, ?)",
                params![parent_taxon_id, rank.as_str(), geological_range],
            )?;
            transaction.last_insert_rowid()
        }
    };
    for name in &plan.names {
        execute_name_plan(transaction, taxon_id, name)?;
    }
    for kind in [NameKind::Scientific, NameKind::English, NameKind::Chinese] {
        if plan
            .names
            .iter()
            .any(|name| name.kind.table() == kind.table())
        {
            validate_accepted_name(transaction, taxon_id, kind)?;
        }
    }
    let target = candidate_from_id(transaction, taxon_id)?;
    let status = if plan.changes.is_empty() {
        TaxonRowStatus::NoChange
    } else {
        TaxonRowStatus::Ready
    };
    Ok(TaxonRowOutcome {
        row_number,
        status,
        message: if status == TaxonRowStatus::NoChange {
            "no change".into()
        } else {
            "ready".into()
        },
        target: Some(target),
        candidates: Vec::new(),
        changes: plan.changes,
    })
}

fn execute_name_plan(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    plan: &NamePlan,
) -> CoreResult<()> {
    if let Some(old_name) = plan.demote_accepted.as_ref() {
        let sql = format!(
            "UPDATE {} SET is_accepted = 0 WHERE taxon_id = ? AND {} = ?",
            plan.kind.table(),
            plan.kind.column()
        );
        transaction.execute(&sql, params![taxon_id, old_name])?;
    }
    if let Some(record) = plan.insert.as_ref() {
        let sql = format!(
            "INSERT INTO {} (taxon_id, {}, is_accepted, authority_year, category, source) VALUES (?, ?, ?, ?, ?, ?)",
            plan.kind.table(),
            plan.kind.column()
        );
        transaction.execute(
            &sql,
            params![
                taxon_id,
                plan.name,
                i64::from(record.is_accepted),
                record.authority_year,
                record.category,
                record.source
            ],
        )?;
    }
    for update in &plan.updates {
        let sql = format!(
            "UPDATE {} SET {} = ? WHERE taxon_id = ? AND {} = ?",
            plan.kind.table(),
            update.field.as_str(),
            plan.kind.column()
        );
        transaction.execute(&sql, params![update.value, taxon_id, plan.name])?;
    }
    if plan.promote {
        let sql = format!(
            "UPDATE {} SET is_accepted = 1 WHERE taxon_id = ? AND {} = ?",
            plan.kind.table(),
            plan.kind.column()
        );
        transaction.execute(&sql, params![taxon_id, plan.name])?;
    }
    Ok(())
}

fn validate_accepted_name(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: NameKind,
) -> CoreResult<()> {
    let sql = format!(
        "SELECT COUNT(*), COALESCE(SUM(is_accepted), 0) FROM {} WHERE taxon_id = ?",
        kind.table()
    );
    let (total, accepted): (i64, i64) =
        transaction.query_row(&sql, [taxon_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    if total > 0 && accepted != 1 {
        return Err(CoreError::InvalidArgument(format!(
            "{} names must have exactly one accepted value for taxon {taxon_id}",
            kind.table()
        )));
    }
    Ok(())
}

fn find_candidates(
    transaction: &Transaction<'_>,
    target_rank: TaxonRank,
    target_name: &str,
    path: &NormalizedPath,
) -> CoreResult<CandidateSearch> {
    let mut statement = transaction.prepare(
        r#"
        SELECT DISTINCT taxa.taxon_id
        FROM taxa
        JOIN scientific ON scientific.taxon_id = taxa.taxon_id
        WHERE taxa.rank = ? AND scientific.scientific_name = ? COLLATE BINARY
        ORDER BY taxa.taxon_id
        "#,
    )?;
    let rows = statement.query_map(params![target_rank.as_str(), target_name], |row| {
        row.get::<_, i64>(0)
    })?;
    let ids = rows.collect::<Result<Vec<_>, _>>()?;
    let had_name_match = !ids.is_empty();
    let mut candidates = Vec::new();
    for taxon_id in ids {
        let mut matches = true;
        for rank in TaxonRank::ALL.into_iter().take(target_rank.index()) {
            if let Some(name) = path.get(rank)
                && !lineage_has_name(transaction, taxon_id, rank, name)?
            {
                matches = false;
                break;
            }
        }
        if matches {
            candidates.push(candidate_from_id(transaction, taxon_id)?);
        }
    }
    Ok(CandidateSearch {
        had_name_match,
        candidates,
    })
}

struct CandidateSearch {
    had_name_match: bool,
    candidates: Vec<TaxonCandidate>,
}

fn lineage_has_name(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    rank: TaxonRank,
    name: &str,
) -> CoreResult<bool> {
    Ok(transaction.query_row(
        r#"
        WITH RECURSIVE lineage(taxon_id, parent_taxon_id, rank) AS (
            SELECT taxon_id, parent_taxon_id, rank FROM taxa WHERE taxon_id = ?
            UNION ALL
            SELECT parent.taxon_id, parent.parent_taxon_id, parent.rank
            FROM taxa AS parent
            JOIN lineage AS child ON child.parent_taxon_id = parent.taxon_id
        )
        SELECT EXISTS(
            SELECT 1
            FROM lineage
            JOIN scientific ON scientific.taxon_id = lineage.taxon_id
            WHERE lineage.rank = ? AND scientific.scientific_name = ? COLLATE BINARY
        )
        "#,
        params![taxon_id, rank.as_str(), name],
        |row| row.get(0),
    )?)
}

fn candidate_from_id(transaction: &Transaction<'_>, taxon_id: i64) -> CoreResult<TaxonCandidate> {
    let mut current_id = Some(taxon_id);
    let mut visited = HashSet::new();
    let mut path = TaxonPath::default();
    let mut target_rank = None;
    let mut target_name = None;
    while let Some(id) = current_id {
        if !visited.insert(id) {
            return Err(CoreError::InvalidArgument(format!(
                "taxon parent cycle detected at {id}"
            )));
        }
        let (parent_id, rank, name): (Option<i64>, String, String) = transaction.query_row(
            r#"
            SELECT
                taxa.parent_taxon_id,
                taxa.rank,
                COALESCE(
                    (SELECT scientific_name FROM scientific
                     WHERE scientific.taxon_id = taxa.taxon_id AND is_accepted = 1),
                    (SELECT scientific_name FROM scientific
                     WHERE scientific.taxon_id = taxa.taxon_id ORDER BY scientific_name LIMIT 1),
                    ''
                )
            FROM taxa WHERE taxa.taxon_id = ?
            "#,
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let rank = parse_rank(&rank)?;
        if id == taxon_id {
            target_rank = Some(rank);
            target_name = Some(name.clone());
        }
        path.set(rank, name);
        current_id = parent_id;
    }
    Ok(TaxonCandidate {
        taxon_id,
        rank: target_rank
            .ok_or_else(|| CoreError::InvalidArgument(format!("taxon {taxon_id} has no rank")))?,
        scientific_name: target_name.unwrap_or_default(),
        path,
    })
}

fn accepted_name(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: NameKind,
) -> rusqlite::Result<Option<String>> {
    let sql = format!(
        "SELECT {} FROM {} WHERE taxon_id = ? AND is_accepted = 1",
        kind.column(),
        kind.table()
    );
    transaction
        .query_row(&sql, [taxon_id], |row| row.get(0))
        .optional()
}

fn count_names(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: NameKind,
) -> rusqlite::Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {} WHERE taxon_id = ?", kind.table());
    transaction.query_row(&sql, [taxon_id], |row| row.get(0))
}

fn required_name(input: &TaxonNameInput) -> Result<String, RowIssue> {
    normalize(Some(&input.name)).ok_or_else(|| {
        RowIssue::new(
            TaxonRowStatus::Invalid,
            "a supplied name group must include a non-empty name",
        )
    })
}

fn normalize(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse_rank(value: &str) -> CoreResult<TaxonRank> {
    match value {
        "kingdom" => Ok(TaxonRank::Kingdom),
        "order" => Ok(TaxonRank::Order),
        "family" => Ok(TaxonRank::Family),
        "genus" => Ok(TaxonRank::Genus),
        "species" => Ok(TaxonRank::Species),
        value => Err(CoreError::InvalidArgument(format!(
            "invalid taxon rank: {value}"
        ))),
    }
}

fn database_issue(error: rusqlite::Error) -> RowIssue {
    RowIssue::new(TaxonRowStatus::Invalid, format!("database error: {error}"))
}

fn core_issue(error: CoreError) -> RowIssue {
    RowIssue::new(TaxonRowStatus::Invalid, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> (tempfile::TempDir, Database) {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(directory.path().join("vividarium.db")).unwrap();
        (directory, database)
    }

    fn seed_lineage(database: &Database) -> [i64; 4] {
        let mut connection = database.connect().unwrap();
        let transaction = connection.transaction().unwrap();
        let mut ids = [0; 4];
        let mut parent = None;
        for (index, (rank, name)) in [
            ("kingdom", "Animalia"),
            ("order", "Carnivora"),
            ("family", "Canidae"),
            ("genus", "Canis"),
        ]
        .into_iter()
        .enumerate()
        {
            transaction
                .execute(
                    "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, ?)",
                    params![parent, rank],
                )
                .unwrap();
            let id = transaction.last_insert_rowid();
            transaction
                .execute(
                    "INSERT INTO scientific (taxon_id, scientific_name, is_accepted) VALUES (?, ?, 1)",
                    params![id, name],
                )
                .unwrap();
            ids[index] = id;
            parent = Some(id);
        }
        transaction.commit().unwrap();
        ids
    }

    fn species_row() -> TaxonInputRow {
        TaxonInputRow {
            species: Some("Canis lupus".into()),
            ..TaxonInputRow::default()
        }
    }

    #[test]
    fn creates_a_species_and_derives_its_genus() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        let result = apply_taxon_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(result.committed);
        assert_eq!(result.rows[0].status, TaxonRowStatus::Ready);
        let connection = database.connect().unwrap();
        let parent: i64 = connection
            .query_row(
                "SELECT parent_taxon_id FROM taxa WHERE rank = 'species'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(parent, ids[3]);
        let accepted: i64 = connection
            .query_row(
                "SELECT is_accepted FROM scientific WHERE scientific_name = 'Canis lupus'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accepted, 1);
    }

    #[test]
    fn requires_the_immediate_parent_for_non_species_taxa() {
        let (_directory, database) = database();
        let result = preview_taxon_rows(
            &database,
            &[TaxonInputRow {
                genus: Some("Canis".into()),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert_eq!(result.rows[0].status, TaxonRowStatus::Invalid);
        assert!(result.rows[0].message.contains("family"));
    }

    #[test]
    fn filters_duplicate_names_with_coarse_ranks() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        let connection = database.connect().unwrap();
        connection
            .execute(
                "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, 'species')",
                [ids[3]],
            )
            .unwrap();
        let first = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO scientific (taxon_id, scientific_name, is_accepted) VALUES (?, 'Canis lupus', 1)",
                [first],
            )
            .unwrap();
        connection
            .execute("INSERT INTO taxa (rank) VALUES ('genus')", [])
            .unwrap();
        let other_genus = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO scientific (taxon_id, scientific_name, is_accepted) VALUES (?, 'Canis', 1)",
                [other_genus],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, 'species')",
                [other_genus],
            )
            .unwrap();
        let second = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO scientific (taxon_id, scientific_name, is_accepted) VALUES (?, 'Canis lupus', 1)",
                [second],
            )
            .unwrap();
        drop(connection);

        let ambiguous =
            preview_taxon_rows(&database, &[species_row()], TaxonUpdateOptions::default()).unwrap();
        assert_eq!(ambiguous.rows[0].status, TaxonRowStatus::Ambiguous);

        let filtered = preview_taxon_rows(
            &database,
            &[TaxonInputRow {
                family: Some("Canidae".into()),
                genus: Some("Canis".into()),
                species: Some("Canis lupus".into()),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert_eq!(filtered.rows[0].status, TaxonRowStatus::NoChange);
        assert_eq!(filtered.rows[0].target.as_ref().unwrap().taxon_id, first);
    }

    #[test]
    fn does_not_create_a_duplicate_when_coarse_filters_are_wrong() {
        let (_directory, database) = database();
        let ids = seed_lineage(&database);
        let connection = database.connect().unwrap();
        connection
            .execute(
                "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, 'species')",
                [ids[3]],
            )
            .unwrap();
        let species_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO scientific (taxon_id, scientific_name, is_accepted) VALUES (?, 'Canis lupus', 1)",
                [species_id],
            )
            .unwrap();
        drop(connection);

        let result = apply_taxon_rows(
            &database,
            &[TaxonInputRow {
                family: Some("Felidae".into()),
                genus: Some("Canis".into()),
                species: Some("Canis lupus".into()),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(!result.committed);
        assert_eq!(result.rows[0].status, TaxonRowStatus::Conflict);
        let connection = database.connect().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM scientific WHERE scientific_name = 'Canis lupus'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn appends_names_only_when_the_permission_is_enabled() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_taxon_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let row = TaxonInputRow {
            species: Some("Canis lupus".into()),
            chinese: Some(TaxonNameInput {
                name: "wolf".into(),
                ..TaxonNameInput::default()
            }),
            ..TaxonInputRow::default()
        };
        let blocked = apply_taxon_rows(
            &database,
            std::slice::from_ref(&row),
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert!(!blocked.committed);
        assert_eq!(blocked.rows[0].status, TaxonRowStatus::Conflict);

        let applied = apply_taxon_rows(
            &database,
            &[row],
            TaxonUpdateOptions {
                allow_new_names: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(applied.committed);
        let connection = database.connect().unwrap();
        let accepted: i64 = connection
            .query_row("SELECT is_accepted FROM chinese", [], |row| row.get(0))
            .unwrap();
        assert_eq!(accepted, 1);
    }

    #[test]
    fn overwrites_only_when_the_permission_is_enabled() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_taxon_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let row = TaxonInputRow {
            species: Some("Canis lupus".into()),
            geological_range: Some("Holocene".into()),
            ..TaxonInputRow::default()
        };
        let blocked = apply_taxon_rows(
            &database,
            std::slice::from_ref(&row),
            TaxonUpdateOptions::default(),
        )
        .unwrap();
        assert_eq!(blocked.rows[0].status, TaxonRowStatus::Conflict);
        let applied = apply_taxon_rows(
            &database,
            &[row],
            TaxonUpdateOptions {
                allow_overwrite: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(applied.committed);
        let connection = database.connect().unwrap();
        let value: String = connection
            .query_row(
                "SELECT geological_range FROM taxa WHERE rank = 'species'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, "Holocene");
    }

    #[test]
    fn switches_the_accepted_name_atomically() {
        let (_directory, database) = database();
        seed_lineage(&database);
        apply_taxon_rows(
            &database,
            &[species_row()],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        let result = apply_taxon_rows(
            &database,
            &[TaxonInputRow {
                species: Some("Canis lupus".into()),
                scientific: Some(TaxonNameInput {
                    name: "Canis lycaon".into(),
                    is_accepted: Some(true),
                    ..TaxonNameInput::default()
                }),
                ..TaxonInputRow::default()
            }],
            TaxonUpdateOptions {
                allow_new_names: true,
                allow_overwrite: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(result.committed);
        let connection = database.connect().unwrap();
        let accepted: String = connection
            .query_row(
                "SELECT scientific_name FROM scientific WHERE is_accepted = 1 AND taxon_id = (SELECT taxon_id FROM scientific WHERE scientific_name = 'Canis lupus')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accepted, "Canis lycaon");
    }

    #[test]
    fn rolls_back_the_entire_batch_when_any_row_is_blocked() {
        let (_directory, database) = database();
        seed_lineage(&database);
        let result = apply_taxon_rows(
            &database,
            &[
                species_row(),
                TaxonInputRow {
                    genus: Some("Vulpes".into()),
                    ..TaxonInputRow::default()
                },
            ],
            TaxonUpdateOptions {
                allow_new_taxa: true,
                ..TaxonUpdateOptions::default()
            },
        )
        .unwrap();
        assert!(!result.committed);
        let connection = database.connect().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM taxa WHERE rank = 'species'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
