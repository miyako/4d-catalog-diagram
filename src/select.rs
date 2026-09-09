//! Subset selection: explicit lists, regex, and relation-graph neighbourhoods.

use std::collections::{BTreeSet, HashSet, VecDeque};

use crate::error::{AppError, Result};
use crate::model::Catalog;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExternalRefs {
    /// Draw a stub node for tables just outside the selection.
    #[default]
    Ghost,
    /// Drop relations that leave the selection entirely.
    Hide,
    /// Pull the external tables into the selection.
    Include,
}

impl ExternalRefs {
    pub fn parse(value: &str) -> Result<ExternalRefs> {
        match value {
            "ghost" => Ok(ExternalRefs::Ghost),
            "hide" => Ok(ExternalRefs::Hide),
            "include" => Ok(ExternalRefs::Include),
            other => Err(AppError::Usage(format!(
                "invalid --external-refs {other:?} (expected ghost, hide, or include)"
            ))),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SelectionSpec {
    pub tables: Vec<String>,
    pub tables_match: Option<String>,
    pub focus: Option<String>,
    pub field: Option<String>,
    pub depth: usize,
    pub external_refs: ExternalRefs,
    pub max_tables: usize,
}

impl SelectionSpec {
    fn is_unfiltered(&self) -> bool {
        self.tables.is_empty()
            && self.tables_match.is_none()
            && self.focus.is_none()
            && self.field.is_none()
    }
}

/// One endpoint of a rendered relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// Index into [`Selection::tables`].
    Table(usize),
    /// Index into [`Selection::ghosts`].
    Ghost(usize),
}

#[derive(Debug, Clone)]
pub struct SelectedRelation {
    /// Index into `Catalog::relations`.
    pub relation: usize,
    pub from: Endpoint,
    pub to: Endpoint,
}

#[derive(Debug, Clone, Default)]
pub struct Selection {
    /// Indices into `Catalog::tables`, in catalog order (deterministic).
    pub tables: Vec<usize>,
    /// Names of tables referenced from the selection but not part of it.
    pub ghosts: Vec<String>,
    pub relations: Vec<SelectedRelation>,
}

impl Selection {
    pub fn position_of(&self, table_index: usize) -> Option<usize> {
        self.tables.iter().position(|&i| i == table_index)
    }
}

/// Undirected adjacency over the relation graph, keyed by table name so
/// relations pointing at unknown tables simply never match.
fn neighbours(catalog: &Catalog, table: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for relation in &catalog.relations {
        if relation.from_table == table {
            out.insert(relation.to_table.clone());
        }
        if relation.to_table == table {
            out.insert(relation.from_table.clone());
        }
    }
    out
}

fn expand(catalog: &Catalog, seeds: BTreeSet<String>, depth: usize) -> BTreeSet<String> {
    let mut seen: BTreeSet<String> = seeds.clone();
    let mut queue: VecDeque<(String, usize)> = seeds.into_iter().map(|n| (n, 0)).collect();
    while let Some((name, hops)) = queue.pop_front() {
        if hops >= depth {
            continue;
        }
        for next in neighbours(catalog, &name) {
            if seen.insert(next.clone()) {
                queue.push_back((next, hops + 1));
            }
        }
    }
    seen
}

pub fn select(catalog: &Catalog, spec: &SelectionSpec) -> Result<Selection> {
    let mut names: BTreeSet<String> = BTreeSet::new();

    if spec.is_unfiltered() {
        names.extend(catalog.tables.iter().map(|t| t.name.clone()));
    }

    for name in &spec.tables {
        if catalog.table(name).is_none() {
            return Err(AppError::Selection(format!(
                "--tables references {name:?}, which is not in this catalog"
            )));
        }
        names.insert(name.clone());
    }

    if let Some(pattern) = &spec.tables_match {
        let re = regex::Regex::new(pattern)
            .map_err(|e| AppError::Usage(format!("invalid --tables-match regex: {e}")))?;
        let matched: Vec<&str> = catalog
            .tables
            .iter()
            .filter(|t| re.is_match(&t.name))
            .map(|t| t.name.as_str())
            .collect();
        if matched.is_empty() {
            return Err(AppError::Selection(format!(
                "--tables-match {pattern:?} matched no table in this catalog"
            )));
        }
        names.extend(matched.into_iter().map(str::to_string));
    }

    if let Some(focus) = &spec.focus {
        if catalog.table(focus).is_none() {
            return Err(AppError::Selection(format!(
                "--focus references {focus:?}, which is not in this catalog"
            )));
        }
        let seeds: BTreeSet<String> = [focus.clone()].into_iter().collect();
        names.extend(expand(catalog, seeds, spec.depth));
    }

    if let Some(spec_field) = &spec.field {
        names.extend(select_by_field(catalog, spec_field, spec.depth)?);
    }

    // Relations may name tables that do not exist in the catalog at all; those
    // can only ever be ghosts.
    names.retain(|n| catalog.table(n).is_some());

    if spec.external_refs == ExternalRefs::Include {
        let mut extra = BTreeSet::new();
        for relation in &catalog.relations {
            let from_in = names.contains(&relation.from_table);
            let to_in = names.contains(&relation.to_table);
            if from_in != to_in {
                let outside = if from_in {
                    &relation.to_table
                } else {
                    &relation.from_table
                };
                if catalog.table(outside).is_some() {
                    extra.insert(outside.clone());
                }
            }
        }
        names.extend(extra);
    }

    if names.is_empty() {
        return Err(AppError::Selection(
            "the selection is empty; loosen --tables/--tables-match/--focus or increase --depth"
                .into(),
        ));
    }
    if names.len() > spec.max_tables {
        return Err(AppError::Selection(format!(
            "the selection contains {} tables, above the --max-tables cap of {}; narrow it with --focus, --tables, or --tables-match, or raise --max-tables",
            names.len(),
            spec.max_tables
        )));
    }

    // Catalog order keeps output stable regardless of how the set was built.
    let tables: Vec<usize> = (0..catalog.tables.len())
        .filter(|&i| names.contains(&catalog.tables[i].name))
        .collect();

    let mut ghosts: Vec<String> = Vec::new();
    let mut relations = Vec::new();
    for (index, relation) in catalog.relations.iter().enumerate() {
        let from_in = names.contains(&relation.from_table);
        let to_in = names.contains(&relation.to_table);
        if !from_in && !to_in {
            continue;
        }
        if from_in != to_in && spec.external_refs == ExternalRefs::Hide {
            continue;
        }

        let mut endpoint = |name: &str, inside: bool| -> Endpoint {
            if inside {
                let table_index = catalog.table_index(name).expect("checked above");
                Endpoint::Table(
                    tables
                        .iter()
                        .position(|&i| i == table_index)
                        .expect("in set"),
                )
            } else {
                let position = ghosts.iter().position(|g| g == name).unwrap_or_else(|| {
                    ghosts.push(name.to_string());
                    ghosts.len() - 1
                });
                Endpoint::Ghost(position)
            }
        };

        let from = endpoint(&relation.from_table, from_in);
        let to = endpoint(&relation.to_table, to_in);
        relations.push(SelectedRelation {
            relation: index,
            from,
            to,
        });
    }

    Ok(Selection {
        tables,
        ghosts,
        relations,
    })
}

/// `--field Table.Field`: start from the one table, then follow only relations
/// that actually touch that field; further hops behave like `--focus`.
fn select_by_field(catalog: &Catalog, spec: &str, depth: usize) -> Result<BTreeSet<String>> {
    let (table_name, field_name) = spec
        .split_once('.')
        .ok_or_else(|| AppError::Usage(format!("--field expects TABLE.FIELD, got {spec:?}")))?;
    let table = catalog.table(table_name).ok_or_else(|| {
        AppError::Selection(format!("--field references unknown table {table_name:?}"))
    })?;
    if !table.fields.iter().any(|f| f.name == field_name) {
        return Err(AppError::Selection(format!(
            "--field references unknown field {table_name}.{field_name}"
        )));
    }

    let mut names: BTreeSet<String> = [table_name.to_string()].into_iter().collect();
    if depth == 0 {
        return Ok(names);
    }

    let mut first_hop = BTreeSet::new();
    for relation in &catalog.relations {
        if relation.from_table == table_name && relation.from_field == field_name {
            first_hop.insert(relation.to_table.clone());
        }
        if relation.to_table == table_name && relation.to_field == field_name {
            first_hop.insert(relation.from_table.clone());
        }
    }
    names.extend(first_hop.clone());

    if depth > 1 {
        names.extend(expand(catalog, first_hop, depth - 1));
    }
    Ok(names)
}

/// Names of tables in the selection, for diagnostics and tests.
pub fn selected_names(catalog: &Catalog, selection: &Selection) -> HashSet<String> {
    selection
        .tables
        .iter()
        .map(|&i| catalog.tables[i].name.clone())
        .collect()
}
