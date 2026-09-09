use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::metadata::{self, MetadataKey};
use crate::naming::TaxonomicNameInfo;
use crate::taxonomy::{TaxonRank, TaxonomyNameType};
use crate::{CoreError, CoreResult, Database};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PhotoNameField {
    SpeciesSci,
    SpeciesZh,
    GenusSci,
    GenusZh,
    FamilySci,
    FamilyZh,
}

impl PhotoNameField {
    pub(crate) const ALL: [Self; 6] = [
        Self::SpeciesSci,
        Self::SpeciesZh,
        Self::GenusSci,
        Self::GenusZh,
        Self::FamilySci,
        Self::FamilyZh,
    ];

    pub(crate) fn value(self, info: &TaxonomicNameInfo) -> Option<&str> {
        match self {
            Self::SpeciesSci => info.species_sci.as_deref(),
            Self::SpeciesZh => info.species_zh.as_deref(),
            Self::GenusSci => info.genus_sci.as_deref(),
            Self::GenusZh => info.genus_zh.as_deref(),
            Self::FamilySci => info.family_sci.as_deref(),
            Self::FamilyZh => info.family_zh.as_deref(),
        }
    }

    pub(crate) fn rank(self) -> TaxonRank {
        match self {
            Self::SpeciesSci | Self::SpeciesZh => TaxonRank::Species,
            Self::GenusSci | Self::GenusZh => TaxonRank::Genus,
            Self::FamilySci | Self::FamilyZh => TaxonRank::Family,
        }
    }

    pub(crate) fn accepted_name_type(self) -> TaxonomyNameType {
        match self {
            Self::SpeciesSci | Self::GenusSci | Self::FamilySci => TaxonomyNameType::SciName,
            Self::SpeciesZh | Self::GenusZh | Self::FamilyZh => TaxonomyNameType::ZhName,
        }
    }

    pub(crate) fn alias_name_type(self) -> TaxonomyNameType {
        match self {
            Self::SpeciesSci | Self::GenusSci | Self::FamilySci => TaxonomyNameType::Synonym,
            Self::SpeciesZh | Self::GenusZh | Self::FamilyZh => TaxonomyNameType::ZhAlias,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoNameMatchSettings {
    pub priority: Vec<PhotoNameField>,
}

impl Default for PhotoNameMatchSettings {
    fn default() -> Self {
        Self {
            priority: PhotoNameField::ALL.to_vec(),
        }
    }
}

pub fn get_photo_name_match_settings(database: &Database) -> CoreResult<PhotoNameMatchSettings> {
    load(&database.connect()?)
}

pub fn set_photo_name_match_settings(
    database: &Database,
    settings: &PhotoNameMatchSettings,
) -> CoreResult<()> {
    validate(settings)?;
    metadata::set_json(
        &database.connect()?,
        MetadataKey::PhotoNameMatchSettings,
        settings,
    )
}

pub(crate) fn load(connection: &rusqlite::Connection) -> CoreResult<PhotoNameMatchSettings> {
    let settings =
        metadata::get_json(connection, MetadataKey::PhotoNameMatchSettings)?.unwrap_or_default();
    validate(&settings)?;
    Ok(settings)
}

fn validate(settings: &PhotoNameMatchSettings) -> CoreResult<()> {
    let values = settings.priority.iter().copied().collect::<HashSet<_>>();
    if settings.priority.len() != PhotoNameField::ALL.len()
        || values.len() != PhotoNameField::ALL.len()
        || !PhotoNameField::ALL
            .into_iter()
            .all(|field| values.contains(&field))
    {
        return Err(CoreError::InvalidArgument(
            "photo name priority must contain each field exactly once".into(),
        ));
    }
    Ok(())
}
