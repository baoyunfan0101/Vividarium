use serde::{Deserialize, Serialize};

use crate::metadata::{self, MetadataKey};
use crate::naming::{PhotoFilenameParser, TaxonomicNameInfo};
use crate::{CoreError, CoreResult, Database};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PhotoFilenameFormatSettings {
    pub family_zh: bool,
    pub family_sci: bool,
    pub genus_zh: bool,
    pub genus_sci: bool,
    pub species_zh: bool,
    pub species_sci: bool,
}

impl Default for PhotoFilenameFormatSettings {
    fn default() -> Self {
        Self {
            family_zh: false,
            family_sci: false,
            genus_zh: false,
            genus_sci: false,
            species_zh: false,
            species_sci: true,
        }
    }
}

pub fn get_photo_filename_format_settings(
    database: &Database,
) -> CoreResult<PhotoFilenameFormatSettings> {
    load_settings(&database.connect()?)
}

pub fn set_photo_filename_format_settings(
    database: &Database,
    settings: &PhotoFilenameFormatSettings,
) -> CoreResult<()> {
    if !settings.any_enabled() {
        return Err(CoreError::InvalidArgument(
            "at least one photo filename field must be enabled".into(),
        ));
    }
    metadata::set_json(
        &database.connect()?,
        MetadataKey::PhotoFilenameFormatSettings,
        settings,
    )
}

pub fn format_photo_filename(
    info: &TaxonomicNameInfo,
    suffix: &str,
    settings: &PhotoFilenameFormatSettings,
) -> CoreResult<String> {
    if suffix.contains('/') || suffix.contains('\\') {
        return Err(CoreError::InvalidArgument(
            "photo filename suffix cannot contain a path".into(),
        ));
    }
    let names = [
        [info.family_zh.as_deref(), info.family_sci.as_deref()],
        [info.genus_zh.as_deref(), info.genus_sci.as_deref()],
        [info.species_zh.as_deref(), info.species_sci.as_deref()],
    ];
    let mut selected = [
        [settings.family_zh, settings.family_sci],
        [settings.genus_zh, settings.genus_sci],
        [settings.species_zh, settings.species_sci],
    ];
    for rank in (1..=2).rev() {
        if names[rank][0].is_none() && names[rank][1].is_none() {
            selected[rank - 1][0] |= selected[rank][0];
            selected[rank - 1][1] |= selected[rank][1];
            selected[rank] = [false, false];
        }
    }
    for rank in 0..3 {
        if selected[rank][0] && names[rank][0].is_none() {
            selected[rank][0] = false;
            selected[rank][1] = true;
        }
        if selected[rank][1] && names[rank][1].is_none() {
            selected[rank][1] = false;
            selected[rank][0] = true;
        }
    }

    let mut output = String::new();
    for rank in 0..3 {
        if selected[rank][0]
            && let Some(name) = names[rank][0]
        {
            output.push_str(name);
        }
        if selected[rank][1]
            && let Some(name) = names[rank][1]
        {
            if !output.is_empty() {
                output.push(' ');
            }
            output.push_str(name);
        }
    }
    if output.is_empty() {
        return Err(CoreError::InvalidArgument(
            "selected photo filename fields contain no names".into(),
        ));
    }
    output.push_str(suffix);
    Ok(output)
}

pub(super) fn filename_for_taxon(
    connection: &rusqlite::Connection,
    taxon_id: i64,
    current_filename: &str,
) -> CoreResult<String> {
    let parser = PhotoFilenameParser::load(connection)?;
    let suffix = parser.parse(current_filename)?.suffix;
    let info = load_taxonomic_name_info(connection, taxon_id)?;
    let settings = load_settings(connection)?;
    format_photo_filename(&info, &suffix, &settings)
}

fn load_settings(connection: &rusqlite::Connection) -> CoreResult<PhotoFilenameFormatSettings> {
    let settings: PhotoFilenameFormatSettings =
        metadata::get_json(connection, MetadataKey::PhotoFilenameFormatSettings)?
            .unwrap_or_default();
    if !settings.any_enabled() {
        return Err(CoreError::InvalidArgument(
            "at least one photo filename field must be enabled".into(),
        ));
    }
    Ok(settings)
}

fn load_taxonomic_name_info(
    connection: &rusqlite::Connection,
    taxon_id: i64,
) -> CoreResult<TaxonomicNameInfo> {
    let mut statement = connection.prepare(
        r#"
        WITH RECURSIVE lineage(taxon_id, parent_taxon_id, rank) AS (
            SELECT taxon_id, parent_taxon_id, rank FROM taxa WHERE taxon_id = ?
            UNION ALL
            SELECT parent.taxon_id, parent.parent_taxon_id, parent.rank
            FROM taxa AS parent
            JOIN lineage AS child ON child.parent_taxon_id = parent.taxon_id
        )
        SELECT lineage.rank, taxon_names.name_type, taxon_names.name
        FROM lineage
        JOIN taxon_names USING (taxon_id)
        WHERE lineage.rank IN (3, 4, 5)
          AND taxon_names.name_type IN (1, 3)
        ORDER BY lineage.rank, taxon_names.name_type
        "#,
    )?;
    let rows = statement.query_map([taxon_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut info = TaxonomicNameInfo::default();
    for row in rows {
        let (rank, name_type, name) = row?;
        match (rank, name_type) {
            (3, 1) => info.family_sci = Some(name),
            (3, 3) => info.family_zh = Some(name),
            (4, 1) => info.genus_sci = Some(name),
            (4, 3) => info.genus_zh = Some(name),
            (5, 1) => info.species_sci = Some(name),
            (5, 3) => info.species_zh = Some(name),
            _ => {}
        }
    }
    if info == TaxonomicNameInfo::default() {
        return Err(CoreError::NotFound(format!("taxon {taxon_id}")));
    }
    Ok(info)
}

impl PhotoFilenameFormatSettings {
    fn any_enabled(&self) -> bool {
        self.family_zh
            || self.family_sci
            || self.genus_zh
            || self.genus_sci
            || self.species_zh
            || self.species_sci
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_reloads_photo_filename_format_settings() {
        let directory = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("unchanged.jpg"), b"photo").unwrap();
        let database = Database::open(directory.path().join("vividarium.db")).unwrap();
        let library =
            crate::photos::open_library(&database, root.path().to_str().unwrap()).unwrap();
        crate::photos::refresh_directory(&database, library.root_directory_id).unwrap();
        let settings = PhotoFilenameFormatSettings {
            family_zh: true,
            family_sci: true,
            genus_zh: false,
            genus_sci: true,
            species_zh: false,
            species_sci: false,
        };

        set_photo_filename_format_settings(&database, &settings).unwrap();

        assert_eq!(
            get_photo_filename_format_settings(&database).unwrap(),
            settings
        );
        assert_eq!(
            crate::photos::list_photos(&database).unwrap()[0].filename,
            "unchanged.jpg"
        );
        assert!(root.path().join("unchanged.jpg").is_file());
    }

    #[test]
    fn formats_selected_fields_and_falls_back_to_available_rank_and_language() {
        let info = TaxonomicNameInfo {
            family_sci: Some("Canidae".into()),
            family_zh: Some("dog family".into()),
            genus_sci: Some("Canis".into()),
            ..TaxonomicNameInfo::default()
        };
        assert_eq!(
            format_photo_filename(&info, "020.jpg", &PhotoFilenameFormatSettings::default())
                .unwrap(),
            "Canis020.jpg"
        );
        assert_eq!(
            format_photo_filename(
                &info,
                ".jpg",
                &PhotoFilenameFormatSettings {
                    genus_zh: true,
                    species_sci: false,
                    ..PhotoFilenameFormatSettings::default()
                }
            )
            .unwrap(),
            "Canis.jpg"
        );
    }
}
