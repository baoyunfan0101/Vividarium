use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SqlColumn {
    pub name: String,
    pub declared_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(String),
}

impl SqlValue {
    pub(super) fn csv_value(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Integer(value) => value.to_string(),
            Self::Real(value) => value.to_string(),
            Self::Text(value) | Self::Blob(value) => value.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SqlStatementMessage {
    pub statement_index: usize,
    pub affected_rows: Option<u64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SqlSourceSchema {
    pub alias: String,
    pub objects: Vec<SqlSourceObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SqlSourceObject {
    pub name: String,
    pub object_type: SqlObjectType,
    pub columns: Vec<SqlColumn>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SqlObjectType {
    Table,
    View,
    VirtualTable,
}
