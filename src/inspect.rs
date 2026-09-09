//! `inspect` — dump the parsed catalog as JSON, with no rendering.
//!
//! This is the cheap path an agent should use to decide *what* is worth
//! rendering. Field names are stable and arrays are sorted by name so output
//! diffs cleanly between runs.

use serde::Serialize;

use crate::model::Catalog;

#[derive(Debug, Serialize)]
pub struct FieldJson {
    pub name: String,
    pub type_code: i64,
    pub type_label: String,
    pub unique: bool,
    pub mandatory: bool,
    pub hidden: bool,
    pub indexed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limiting_length: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TableJson {
    pub name: String,
    pub field_count: usize,
    pub has_layout: bool,
    pub primary_key: Option<String>,
    pub relations_in: usize,
    pub relations_out: usize,
    pub fields: Vec<FieldJson>,
}

#[derive(Debug, Serialize)]
#[allow(non_snake_case)]
pub struct RelationJson {
    pub name_Nto1: Option<String>,
    pub name_1toN: Option<String>,
    pub from_table: String,
    pub from_field: String,
    pub to_table: String,
    pub to_field: String,
}

#[derive(Debug, Serialize)]
pub struct AnalysisJson {
    pub tables_without_primary_key: Vec<String>,
    pub isolated_tables: Vec<String>,
    pub most_connected_tables: Vec<ConnectedJson>,
}

#[derive(Debug, Serialize)]
pub struct ConnectedJson {
    pub name: String,
    pub relation_count: usize,
}

#[derive(Debug, Serialize)]
pub struct CatalogJson {
    pub base_name: String,
    pub table_count: usize,
    pub relation_count: usize,
    pub index_count: usize,
    pub tables: Vec<TableJson>,
    pub relations: Vec<RelationJson>,
    pub analysis: AnalysisJson,
    pub warnings: Vec<String>,
}

pub fn to_json(catalog: &Catalog) -> CatalogJson {
    let mut tables: Vec<TableJson> = catalog
        .tables
        .iter()
        .map(|table| {
            let mut fields: Vec<FieldJson> = table
                .fields
                .iter()
                .map(|f| FieldJson {
                    name: f.name.clone(),
                    type_code: f.type_code,
                    type_label: f.type_label(),
                    unique: f.unique,
                    mandatory: f.mandatory || f.never_null,
                    hidden: f.hidden,
                    indexed: f.indexed,
                    limiting_length: f.limiting_length,
                    tip: f.tip.clone(),
                })
                .collect();
            fields.sort_by(|a, b| a.name.cmp(&b.name));
            TableJson {
                name: table.name.clone(),
                field_count: table.fields.len(),
                has_layout: table.coordinates.is_some(),
                primary_key: table.primary_key.clone(),
                relations_in: catalog.relations_in(&table.name),
                relations_out: catalog.relations_out(&table.name),
                fields,
            }
        })
        .collect();
    tables.sort_by(|a, b| a.name.cmp(&b.name));

    let mut relations: Vec<RelationJson> = catalog
        .relations
        .iter()
        .map(|r| RelationJson {
            name_Nto1: r.name_nto1.clone(),
            name_1toN: r.name_1ton.clone(),
            from_table: r.from_table.clone(),
            from_field: r.from_field.clone(),
            to_table: r.to_table.clone(),
            to_field: r.to_field.clone(),
        })
        .collect();
    relations.sort_by(|a, b| {
        (&a.from_table, &a.from_field, &a.to_table, &a.to_field).cmp(&(
            &b.from_table,
            &b.from_field,
            &b.to_table,
            &b.to_field,
        ))
    });

    let mut without_pk: Vec<String> = catalog
        .tables
        .iter()
        .filter(|t| t.primary_key.is_none())
        .map(|t| t.name.clone())
        .collect();
    without_pk.sort();

    let mut isolated: Vec<String> = catalog
        .tables
        .iter()
        .filter(|t| catalog.relations_in(&t.name) + catalog.relations_out(&t.name) == 0)
        .map(|t| t.name.clone())
        .collect();
    isolated.sort();

    let mut connected: Vec<ConnectedJson> = catalog
        .tables
        .iter()
        .map(|t| ConnectedJson {
            name: t.name.clone(),
            relation_count: catalog.relations_in(&t.name) + catalog.relations_out(&t.name),
        })
        .filter(|c| c.relation_count > 0)
        .collect();
    // Descending by count, then by name so ties are stable.
    connected.sort_by(|a, b| {
        b.relation_count
            .cmp(&a.relation_count)
            .then_with(|| a.name.cmp(&b.name))
    });
    connected.truncate(10);

    CatalogJson {
        base_name: catalog.base_name.clone(),
        table_count: catalog.tables.len(),
        relation_count: catalog.relations.len(),
        index_count: catalog.indexes.len(),
        tables,
        relations,
        analysis: AnalysisJson {
            tables_without_primary_key: without_pk,
            isolated_tables: isolated,
            most_connected_tables: connected,
        },
        warnings: catalog.warnings.clone(),
    }
}

pub fn to_string_pretty(catalog: &Catalog) -> String {
    serde_json::to_string_pretty(&to_json(catalog)).expect("catalog JSON is serializable")
}
