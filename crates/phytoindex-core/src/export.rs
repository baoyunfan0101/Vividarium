use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use rusqlite::types::ValueRef;

use crate::db::Database;
use crate::error::{CoreError, CoreResult};

const ALLOWED_TABLES: &[&str] = &[
    "photos",
    "photos_dir",
    "photos_metadata",
    "taxa",
    "scientific",
    "english",
    "chinese",
    "taxon_identifiers",
    "taxonomy_operation_batches",
    "taxonomy_operations",
    "taxa_metadata",
    "photos_taxa_mapping",
    "photos_taxa_mapping_metadata",
    "photos_taxa_mapping_taxa",
];

pub fn export_table(database: &Database, table_name: &str, output: &Path) -> CoreResult<usize> {
    if !ALLOWED_TABLES.contains(&table_name) {
        return Err(CoreError::InvalidArgument(format!(
            "invalid table name: {table_name}"
        )));
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let connection = database.connect()?;
    let mut columns_statement = connection.prepare(&format!("PRAGMA table_info({table_name})"))?;
    let columns = columns_statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut statement = connection.prepare(&format!("SELECT * FROM {table_name}"))?;
    let file = File::create(output)?;
    let mut output = BufWriter::new(file);
    output.write_all(b"\xEF\xBB\xBF")?;
    let mut writer = csv::Writer::from_writer(output);
    writer.write_record(&columns)?;
    let mut rows = statement.query([])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        let mut values = Vec::with_capacity(columns.len());
        for index in 0..columns.len() {
            values.push(value_to_string(row.get_ref(index)?));
        }
        writer.write_record(values)?;
        count += 1;
    }
    writer.flush()?;
    Ok(count)
}

fn value_to_string(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => String::new(),
        ValueRef::Integer(value) => value.to_string(),
        ValueRef::Real(value) => value.to_string(),
        ValueRef::Text(value) => String::from_utf8_lossy(value).into_owned(),
        ValueRef::Blob(value) => value.iter().map(|byte| format!("{byte:02x}")).collect(),
    }
}
