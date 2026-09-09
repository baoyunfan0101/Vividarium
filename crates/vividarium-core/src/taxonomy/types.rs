use serde::{Deserialize, Serialize};

use crate::{CoreError, CoreResult};

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
    pub(crate) const ALL: [Self; 5] = [
        Self::Kingdom,
        Self::Order,
        Self::Family,
        Self::Genus,
        Self::Species,
    ];

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Kingdom => "kingdom",
            Self::Order => "order",
            Self::Family => "family",
            Self::Genus => "genus",
            Self::Species => "species",
        }
    }

    pub(crate) fn code(self) -> i64 {
        self.index() as i64 + 1
    }

    pub(crate) fn from_code(value: i64) -> CoreResult<Self> {
        Self::ALL
            .get(value.saturating_sub(1) as usize)
            .copied()
            .ok_or_else(|| CoreError::InvalidArgument(format!("invalid taxon rank code: {value}")))
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Self::Kingdom => 0,
            Self::Order => 1,
            Self::Family => 2,
            Self::Genus => 3,
            Self::Species => 4,
        }
    }

    pub(super) fn parent(self) -> Option<Self> {
        Self::ALL.get(self.index().wrapping_sub(1)).copied()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaxonomyNameType {
    SciName,
    Synonym,
    ZhName,
    ZhAlias,
    EnName,
    EnAlias,
}

impl TaxonomyNameType {
    pub(crate) const ALL: [Self; 6] = [
        Self::SciName,
        Self::Synonym,
        Self::ZhName,
        Self::ZhAlias,
        Self::EnName,
        Self::EnAlias,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SciName => "sci_name",
            Self::Synonym => "synonym",
            Self::ZhName => "zh_name",
            Self::ZhAlias => "zh_alias",
            Self::EnName => "en_name",
            Self::EnAlias => "en_alias",
        }
    }

    pub fn from_value(value: &str) -> CoreResult<Self> {
        match value {
            "sci_name" => Ok(Self::SciName),
            "synonym" => Ok(Self::Synonym),
            "zh_name" => Ok(Self::ZhName),
            "zh_alias" => Ok(Self::ZhAlias),
            "en_name" => Ok(Self::EnName),
            "en_alias" => Ok(Self::EnAlias),
            _ => Err(CoreError::InvalidArgument(format!(
                "invalid taxonomy name type: {value}"
            ))),
        }
    }

    pub(crate) fn code(self) -> i64 {
        self.index() as i64 + 1
    }

    pub(crate) fn from_code(value: i64) -> CoreResult<Self> {
        Self::ALL
            .get(value.saturating_sub(1) as usize)
            .copied()
            .ok_or_else(|| {
                CoreError::InvalidArgument(format!("invalid taxonomy name type code: {value}"))
            })
    }

    fn index(self) -> usize {
        match self {
            Self::SciName => 0,
            Self::Synonym => 1,
            Self::ZhName => 2,
            Self::ZhAlias => 3,
            Self::EnName => 4,
            Self::EnAlias => 5,
        }
    }

    pub fn is_primary(self) -> bool {
        matches!(self, Self::SciName | Self::ZhName | Self::EnName)
    }

    pub fn accepted_type(self) -> Self {
        match self {
            Self::SciName | Self::Synonym => Self::SciName,
            Self::ZhName | Self::ZhAlias => Self::ZhName,
            Self::EnName | Self::EnAlias => Self::EnName,
        }
    }

    pub fn alias_type(self) -> Self {
        match self {
            Self::SciName | Self::Synonym => Self::Synonym,
            Self::ZhName | Self::ZhAlias => Self::ZhAlias,
            Self::EnName | Self::EnAlias => Self::EnAlias,
        }
    }
}
