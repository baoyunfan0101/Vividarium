use base64::Engine;
use serde::{Deserialize, Serialize};

use super::TaxonRank;
use crate::{CoreError, CoreResult};

const MAX_PAGE_LIMIT: usize = 500;
pub(super) const DEFAULT_PAGE_LIMIT: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum TaxonomyCursor {
    TaxonSearch {
        query: String,
        taxon_id: i64,
    },
    TaxonChildren {
        parent_taxon_id: i64,
        rank: TaxonRank,
        taxon_id: i64,
    },
    OperationBatches {
        created_at: String,
        batch_id: i64,
    },
    Operations {
        operation_id: i64,
    },
    BatchOperations {
        batch_id: i64,
        row_number: usize,
        operation_id: i64,
    },
}

pub(super) fn page_limit(limit: usize) -> usize {
    limit.clamp(1, MAX_PAGE_LIMIT)
}

pub(super) fn encode_cursor(cursor: &TaxonomyCursor) -> CoreResult<String> {
    let value = serde_json::to_vec(cursor)
        .map_err(|error| CoreError::InvalidArgument(error.to_string()))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value))
}

pub(super) fn decode_cursor(value: Option<&str>) -> CoreResult<Option<TaxonomyCursor>> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| CoreError::InvalidArgument("invalid taxonomy cursor".into()))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| CoreError::InvalidArgument("invalid taxonomy cursor".into()))
}

pub(super) fn invalid_cursor() -> CoreError {
    CoreError::InvalidArgument("invalid taxonomy cursor".into())
}
