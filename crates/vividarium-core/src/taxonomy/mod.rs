//! Taxonomy search, detail views, mutations, import workflows, and history.
//!
//! This module is the public taxonomy facade. SQL execution details, session
//! storage, validation, and synchronization remain private implementation
//! details.

mod actions;
mod changeset;
mod cleanup;
mod direct_import;
mod exact_match;
mod formatted;
mod operation_export;
mod page;
mod query;
mod sql;
mod sql_import;
mod sql_inputs;
mod sql_sources;
mod sql_support;
mod sql_types;
pub(crate) mod sync;
mod types;
mod validation;
mod view;

pub use crate::naming::{ScientificNameParts, split_scientific_name_authority};
pub use crate::operations::OperationInput;
pub use actions::{
    DeleteTaxonNameInput, NewTaxonNameInput, PromoteTaxonNameInput, SaveTaxonNameGroupInput,
    TaxonNameMetadataInput, delete_taxon, delete_taxon_name, promote_taxon_name,
    save_taxon_name_group,
};
pub use direct_import::{
    DirectImportDatabase, TaxonomyImportMetadata, TaxonomyImportResult, apply_direct_import,
    apply_direct_import_with_cancellation, apply_direct_import_with_progress_and_cancellation,
    get_taxonomy_import_metadata, inspect_direct_import_database,
};
pub(crate) use exact_match::match_exact_taxonomy_name;
pub use formatted::{
    PreparedTaxonomyUpdate, TaxonChange, TaxonChangeKind, TaxonInputRow, TaxonRowOutcome,
    TaxonRowStatus, TaxonomyOperationResult, TaxonomyPreviewResult, apply_prepared_rows,
    apply_prepared_rows_with_cancellation, apply_rows, get_operation_input,
    get_taxonomy_name_separator, list_operation_audit, list_operations, parse_taxonomy_input_csv,
    prepare_rows, prepare_rows_with_cancellation, preview_rows, rollback_operation,
    set_taxonomy_name_separator, taxonomy_formatted_update_template, taxonomy_log_csv,
};
pub use operation_export::{
    export_all_replayable_inputs, export_operation_input, export_operations_input,
    write_all_operation_audit, write_operation_audit, write_operations_audit,
};
pub use page::TaxonomyPage;
pub use query::{TaxonNameMatch, TaxonSearchResult, TaxonSuggestion, search_taxa, suggest_taxa};
pub(crate) use query::{
    TaxonSearchCursorKey, search_taxa_page_with_photos_connection,
    suggest_taxa_with_photos_connection, taxon_search_relation,
};
pub use sql::{
    CustomSqlExecutionResult, CustomTaxonomySqlExportRequest, CustomTaxonomySqlRequest,
    SqlExportResult, SqlResultSet, add_custom_sql_input, execute_custom_taxonomy_sql,
    execute_custom_taxonomy_sql_with_cancellation,
    execute_custom_taxonomy_sql_with_progress_and_cancellation, export_custom_taxonomy_query,
    export_custom_taxonomy_query_with_cancellation, get_custom_taxonomy_sql,
    list_custom_sql_database_schemas, list_custom_sql_inputs, remove_custom_sql_input,
};
pub use sql_import::{
    NameTypeCount, SqlImportExecutionResult, SqlImportIssue, SqlImportValidationResult,
    ValidateSqlImportRequest, ValidateSqlImportResult, add_sql_import_input, apply_sql_import,
    apply_sql_import_with_cancellation, apply_sql_import_with_progress_and_cancellation,
    get_sql_import_sql, list_sql_import_database_schemas, list_sql_import_inputs,
    list_sql_import_staging_schemas, remove_sql_import_input, validate_sql_import,
    validate_sql_import_with_progress, validate_sql_import_with_progress_and_cancellation,
};
pub use sql_inputs::{
    AddSqlInputRequest, AddSqlInputResult, PersistentSqlInput, RemoveSqlInputRequest,
    RemoveSqlInputResult, SqlInputKind,
};
pub use sql_types::{
    SqlColumn, SqlObjectType, SqlSourceObject, SqlSourceSchema, SqlStatementMessage, SqlValue,
};
pub use sync::{
    TaxonomySyncResult, TaxonomySyncRun, has_pending_photo_library_sync,
    synchronize_pending_photo_libraries,
};
pub use types::{TaxonRank, TaxonomyNameType};
pub(crate) use view::load_taxon_display_summary;
pub(crate) use view::load_taxon_summaries;
pub use view::{
    TaxonBreadcrumbItem, TaxonChild, TaxonDetail, TaxonDisplayItem, TaxonDisplayNames,
    TaxonDisplaySummary, TaxonNameDetail, TaxonNamesDetail, TaxonSummary, get_taxon_detail,
    get_taxon_display_summary, get_taxon_summary, list_taxon_children,
};
