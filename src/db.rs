use crate::error::DbError;
use crate::prep::{self, StockMini};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Debug, Clone)]
struct LocRow {
    id: i64,
    parent_id: Option<i64>,
    name: String,
    kind: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Location {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct NewLocation {
    pub name: String,
    pub kind: String,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Tag {
    pub id: i64,
    pub category: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Miniature {
    pub id: i64,
    pub name: String,
    pub notes: String,
    pub quantity: u32,
    pub size: String,
    pub game: Option<String>,
    pub location_id: i64,
    pub location_path: String,
    pub races: Vec<String>,
    pub roles: Vec<String>,
    pub combat: Vec<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewMiniature {
    pub name: String,
    pub notes: String,
    pub quantity: u32,
    pub size: String,
    pub game: Option<String>,
    pub location_id: i64,
    pub races: Vec<String>,
    pub roles: Vec<String>,
    pub combat: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MiniFilter {
    pub name: Option<String>,
    pub races: Vec<String>,
    pub roles: Vec<String>,
    pub combat: Vec<String>,
    pub sizes: Vec<String>,
    pub games: Vec<String>,
    pub location_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocationReport {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub direct_entries: i64,
    pub direct_quantity: i64,
    pub total_entries: i64,
    pub total_quantity: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Facets {
    pub races: Vec<String>,
    pub roles: Vec<String>,
    pub combat: Vec<String>,
    pub sizes: Vec<String>,
    pub games: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApiKeyRow {
    pub id: i64,
    pub name: String,
    pub scope: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

struct MiniRow {
    id: i64,
    name: String,
    notes: String,
    quantity: i64,
    size: String,
    game: Option<String>,
    location_id: i64,
    thumbnail_file: Option<String>,
}

pub fn open_connection(path: &Path) -> Result<Connection, DbError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| DbError::Internal(format!("create data dir: {err}")))?;
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

pub fn list_locations(conn: &Connection) -> Result<Vec<Location>, DbError> {
    let rows = load_location_rows(conn)?;
    let mut locations = Vec::with_capacity(rows.len());
    for row in &rows {
        let path = path_of(&rows, row.id).ok_or_else(|| {
            DbError::Internal(format!("location {} path could not be built", row.id))
        })?;
        locations.push(Location {
            id: row.id,
            parent_id: row.parent_id,
            name: row.name.clone(),
            kind: row.kind.clone(),
            path,
        });
    }
    locations.sort_by(|left, right| left.path.cmp(&right.path).then(left.id.cmp(&right.id)));
    Ok(locations)
}

pub fn get_location(conn: &Connection, id: i64) -> Result<Option<Location>, DbError> {
    Ok(list_locations(conn)?
        .into_iter()
        .find(|location| location.id == id))
}

pub fn create_location(conn: &Connection, input: NewLocation) -> Result<Location, DbError> {
    let id = insert_location(conn, &input)?;
    get_location(conn, id)?.ok_or(DbError::NotFound)
}

pub fn update_location(
    conn: &Connection,
    id: i64,
    input: NewLocation,
) -> Result<Location, DbError> {
    let name = clean_location_name(&input.name)?;
    let kind = required(&input.kind, "kind", 40)?;
    let rows = load_location_rows(conn)?;
    if !rows.iter().any(|row| row.id == id) {
        return Err(DbError::NotFound);
    }
    if let Some(parent) = input.parent_id {
        if !rows.iter().any(|row| row.id == parent) {
            return Err(DbError::Invalid("parent location does not exist".into()));
        }
    }
    if would_cycle(&rows, id, input.parent_id) {
        return Err(DbError::Invalid(
            "a location cannot be moved inside itself".into(),
        ));
    }
    conn.execute(
        "UPDATE locations SET parent_id = ?1, name = ?2, kind = ?3 WHERE id = ?4",
        params![input.parent_id, name, kind, id],
    )?;
    get_location(conn, id)?.ok_or(DbError::NotFound)
}

pub fn delete_location(conn: &Connection, id: i64) -> Result<(), DbError> {
    let children: i64 = conn.query_row(
        "SELECT COUNT(*) FROM locations WHERE parent_id = ?1",
        [id],
        |row| row.get(0),
    )?;
    if children > 0 {
        let noun = if children == 1 {
            "location is"
        } else {
            "locations are"
        };
        return Err(DbError::Conflict(format!(
            "{children} {noun} inside this one. Move or delete them before deleting it."
        )));
    }
    let miniatures: i64 = conn.query_row(
        "SELECT COUNT(*) FROM miniatures WHERE location_id = ?1",
        [id],
        |row| row.get(0),
    )?;
    if miniatures > 0 {
        let noun = if miniatures == 1 {
            "miniature is"
        } else {
            "miniatures are"
        };
        return Err(DbError::Conflict(format!(
            "{miniatures} {noun} stored here. Move them before deleting this location."
        )));
    }
    let changed = conn.execute("DELETE FROM locations WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub fn list_tags(conn: &Connection) -> Result<Vec<Tag>, DbError> {
    let mut stmt = conn.prepare("SELECT id, category, name FROM tags ORDER BY category, name")?;
    let rows = stmt.query_map([], |row| {
        Ok(Tag {
            id: row.get(0)?,
            category: row.get(1)?,
            name: row.get(2)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
}

pub fn create_tag(conn: &Connection, category: &str, name: &str) -> Result<Tag, DbError> {
    if !matches!(category, "race" | "role" | "combat") {
        return Err(DbError::Invalid(
            "category must be race, role, or combat".into(),
        ));
    }
    let names = normalize_tags(&[name.to_string()])?;
    let Some(name) = names.first() else {
        return Err(DbError::Invalid("tag name is required".into()));
    };
    let id = ensure_tag(conn, category, name)?;
    list_tags(conn)?
        .into_iter()
        .find(|tag| tag.id == id)
        .ok_or(DbError::NotFound)
}

pub fn list_miniatures(conn: &Connection, filter: &MiniFilter) -> Result<Vec<Miniature>, DbError> {
    let locations = list_locations(conn)?;
    let allowed = if filter.location_ids.is_empty() {
        None
    } else {
        Some(subtree_ids(&locations, &filter.location_ids))
    };
    let mut miniatures = load_miniatures(conn, &locations)?;
    miniatures.retain(|mini| {
        allowed
            .as_ref()
            .map(|ids| ids.contains(&mini.location_id))
            .unwrap_or(true)
            && prep::name_matches(&mini.name, filter.name.as_deref().unwrap_or(""))
            && prep::facet_matches(&mini.races, &filter.races)
            && prep::facet_matches(&mini.roles, &filter.roles)
            && prep::facet_matches(&mini.combat, &filter.combat)
            && ci_any(&mini.size, &filter.sizes)
            && game_any(mini.game.as_deref(), &filter.games)
    });
    miniatures.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then(left.id.cmp(&right.id))
    });
    Ok(miniatures)
}

pub fn get_miniature(conn: &Connection, id: i64) -> Result<Option<Miniature>, DbError> {
    Ok(load_miniatures(conn, &list_locations(conn)?)?
        .into_iter()
        .find(|mini| mini.id == id))
}

pub fn validate_miniature(mini: NewMiniature) -> Result<NewMiniature, DbError> {
    if mini.quantity > 100_000 {
        return Err(DbError::Invalid("quantity is too large".into()));
    }
    let notes = mini.notes.trim();
    if notes.chars().count() > 4000 {
        return Err(DbError::Invalid("notes are too long".into()));
    }
    Ok(NewMiniature {
        name: required(&mini.name, "name", 200)?,
        notes: notes.to_string(),
        quantity: mini.quantity,
        size: required(&mini.size, "size", 32)?,
        game: optional_text(mini.game.as_deref(), "game", 120)?,
        location_id: mini.location_id,
        races: normalize_tags(&mini.races)?,
        roles: normalize_tags(&mini.roles)?,
        combat: normalize_tags(&mini.combat)?,
    })
}

pub fn create_miniature(conn: &Connection, mini: NewMiniature) -> Result<Miniature, DbError> {
    let tx = conn.unchecked_transaction()?;
    let id = insert_miniature(&tx, &mini)?;
    tx.commit()?;
    get_miniature(conn, id)?.ok_or(DbError::NotFound)
}

pub fn update_miniature(
    conn: &Connection,
    id: i64,
    mini: NewMiniature,
) -> Result<Miniature, DbError> {
    require_location(conn, mini.location_id)?;
    if get_miniature(conn, id)?.is_none() {
        return Err(DbError::NotFound);
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE miniatures
         SET name = ?1, notes = ?2, quantity = ?3, size = ?4, game = ?5, location_id = ?6
         WHERE id = ?7",
        params![
            mini.name,
            mini.notes,
            mini.quantity,
            mini.size,
            mini.game,
            mini.location_id,
            id
        ],
    )?;
    replace_tags(&tx, id, &mini)?;
    tx.commit()?;
    get_miniature(conn, id)?.ok_or(DbError::NotFound)
}

pub fn delete_miniature(conn: &Connection, id: i64) -> Result<(), DbError> {
    let changed = conn.execute("DELETE FROM miniatures WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub fn set_thumbnail(conn: &Connection, id: i64, file: &str) -> Result<(), DbError> {
    let changed = conn.execute(
        "UPDATE miniatures SET thumbnail_file = ?1 WHERE id = ?2",
        params![file, id],
    )?;
    if changed == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub fn stock(conn: &Connection) -> Result<Vec<StockMini>, DbError> {
    let miniatures = list_miniatures(conn, &MiniFilter::default())?;
    Ok(miniatures
        .into_iter()
        .map(|mini| {
            let mut tags = Vec::new();
            tags.extend(mini.races.iter().cloned());
            tags.extend(mini.roles.iter().cloned());
            tags.extend(mini.combat.iter().cloned());
            StockMini {
                id: mini.id,
                name: mini.name,
                quantity: mini.quantity,
                size: mini.size,
                game: mini.game,
                location_id: mini.location_id,
                location_path: mini.location_path,
                races: mini.races,
                roles: mini.roles,
                combat: mini.combat,
                tags,
            }
        })
        .collect())
}

pub fn location_report(conn: &Connection) -> Result<Vec<LocationReport>, DbError> {
    let locations = list_locations(conn)?;
    let mut direct_entries: HashMap<i64, i64> = HashMap::new();
    let mut direct_quantity: HashMap<i64, i64> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT location_id, COUNT(*), COALESCE(SUM(quantity), 0)
         FROM miniatures GROUP BY location_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (id, entries, quantity) = row?;
        direct_entries.insert(id, entries);
        direct_quantity.insert(id, quantity);
    }

    let mut total_entries: HashMap<i64, i64> = locations
        .iter()
        .map(|location| (location.id, *direct_entries.get(&location.id).unwrap_or(&0)))
        .collect();
    let mut total_quantity: HashMap<i64, i64> = locations
        .iter()
        .map(|location| {
            (
                location.id,
                *direct_quantity.get(&location.id).unwrap_or(&0),
            )
        })
        .collect();
    let mut ordered = locations.clone();
    ordered.sort_by_key(|location| std::cmp::Reverse(location.path.matches(" > ").count()));
    for location in &ordered {
        let entries = total_entries[&location.id];
        let quantity = total_quantity[&location.id];
        if let Some(parent) = location.parent_id {
            if let Some(slot) = total_entries.get_mut(&parent) {
                *slot += entries;
            }
            if let Some(slot) = total_quantity.get_mut(&parent) {
                *slot += quantity;
            }
        }
    }

    Ok(locations
        .into_iter()
        .map(|location| LocationReport {
            direct_entries: *direct_entries.get(&location.id).unwrap_or(&0),
            direct_quantity: *direct_quantity.get(&location.id).unwrap_or(&0),
            total_entries: total_entries[&location.id],
            total_quantity: total_quantity[&location.id],
            id: location.id,
            parent_id: location.parent_id,
            name: location.name,
            kind: location.kind,
            path: location.path,
        })
        .collect())
}

pub fn facets(conn: &Connection) -> Result<Facets, DbError> {
    let miniatures = list_miniatures(conn, &MiniFilter::default())?;
    let tags = list_tags(conn)?;
    let mut races = Vec::new();
    let mut roles = Vec::new();
    let mut combat = Vec::new();
    let mut sizes = Vec::new();
    let mut games = Vec::new();
    for tag in tags {
        match tag.category.as_str() {
            "race" => push_one(&mut races, tag.name),
            "role" => push_one(&mut roles, tag.name),
            "combat" => push_one(&mut combat, tag.name),
            _ => {}
        }
    }
    for mini in miniatures {
        push_one(&mut sizes, mini.size);
        if let Some(game) = mini.game {
            push_one(&mut games, game);
        }
    }
    let mut term_stmt = conn.prepare("SELECT category, name FROM facet_terms")?;
    let term_rows = term_stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in term_rows {
        let (category, name) = row?;
        match category.as_str() {
            "size" => push_one(&mut sizes, name),
            "game" => push_one(&mut games, name),
            _ => {}
        }
    }
    for values in [&mut races, &mut roles, &mut combat, &mut sizes, &mut games] {
        values.sort();
    }
    Ok(Facets {
        races,
        roles,
        combat,
        sizes,
        games,
    })
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FilterEntry {
    pub name: String,
    pub used_by: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FilterGroup {
    pub category: String,
    pub label: String,
    pub entries: Vec<FilterEntry>,
}

pub fn list_filters(conn: &Connection) -> Result<Vec<FilterGroup>, DbError> {
    Ok(vec![
        tag_group(conn, "race", "Race / type")?,
        tag_group(conn, "combat", "Combat style")?,
        facet_group(conn, "size", "Size")?,
        tag_group(conn, "role", "Role")?,
        facet_group(conn, "game", "Game")?,
        location_group(conn)?,
    ])
}

pub fn add_filter(
    conn: &Connection,
    category: &str,
    name: &str,
    kind: Option<&str>,
    parent_id: Option<i64>,
) -> Result<FilterEntry, DbError> {
    match category {
        "race" | "role" | "combat" => add_tag_filter(conn, category, name),
        "size" | "game" => add_facet_filter(conn, category, name),
        "location" => add_location_filter(conn, name, kind.unwrap_or(""), parent_id),
        _ => Err(unknown_category()),
    }
}

pub fn rename_filter(
    conn: &Connection,
    category: &str,
    name: &str,
    new_name: &str,
    id: Option<i64>,
) -> Result<FilterEntry, DbError> {
    match category {
        "race" | "role" | "combat" => {
            rename_tag_filter(conn, category, name, new_name)?;
            let stored = single_tag_name(new_name)?;
            tag_entry(conn, category, &stored)
        }
        "size" | "game" => {
            let stored = rename_facet_filter(conn, category, name, new_name)?;
            facet_entry(conn, category, &stored)
        }
        "location" => {
            let id = id.ok_or_else(|| DbError::Invalid("location id is required".into()))?;
            rename_location_filter(conn, id, new_name)
        }
        _ => Err(unknown_category()),
    }
}

pub fn delete_filter(
    conn: &Connection,
    category: &str,
    name: &str,
    id: Option<i64>,
) -> Result<(), DbError> {
    match category {
        "race" | "role" | "combat" => delete_tag_filter(conn, category, name),
        "size" | "game" => delete_facet_filter(conn, category, name),
        "location" => {
            let id = id.ok_or_else(|| DbError::Invalid("location id is required".into()))?;
            delete_location(conn, id)
        }
        _ => Err(unknown_category()),
    }
}

fn unknown_category() -> DbError {
    DbError::Invalid("category must be race, combat, size, role, game, or location".into())
}

fn category_label(category: &str) -> &str {
    match category {
        "combat" => "combat style",
        other => other,
    }
}

fn already_named(label: &str, name: &str) -> String {
    format!("a {label} named \"{name}\" already exists")
}

fn rename_refused(label: &str, name: &str) -> String {
    format!(
        "{}. The rename was not applied.",
        already_named(label, name)
    )
}

fn still_in_use(label: &str, count: i64) -> String {
    if count == 1 {
        format!("1 miniature still uses this {label}. Reassign it before deleting.")
    } else {
        format!("{count} miniatures still use this {label}. Reassign them before deleting.")
    }
}

fn single_tag_name(name: &str) -> Result<String, DbError> {
    let names = normalize_tags(&[name.to_string()])?;
    names
        .into_iter()
        .next()
        .ok_or_else(|| DbError::Invalid("name is required".into()))
}

fn tag_group(conn: &Connection, category: &str, label: &str) -> Result<FilterGroup, DbError> {
    let tags = list_tags(conn)?;
    let mut entries = Vec::new();
    for tag in tags.into_iter().filter(|tag| tag.category == category) {
        entries.push(tag_entry(conn, category, &tag.name)?);
    }
    Ok(FilterGroup {
        category: category.to_string(),
        label: label.to_string(),
        entries,
    })
}

fn tag_entry(conn: &Connection, category: &str, name: &str) -> Result<FilterEntry, DbError> {
    let tag = list_tags(conn)?
        .into_iter()
        .find(|tag| tag.category == category && tag.name == name)
        .ok_or(DbError::NotFound)?;
    let used_by: i64 = conn.query_row(
        "SELECT COUNT(*) FROM miniature_tags WHERE tag_id = ?1",
        [tag.id],
        |row| row.get(0),
    )?;
    Ok(plain_entry(tag.name, used_by))
}

fn add_tag_filter(conn: &Connection, category: &str, name: &str) -> Result<FilterEntry, DbError> {
    let stored = single_tag_name(name)?;
    if list_tags(conn)?
        .iter()
        .any(|tag| tag.category == category && tag.name == stored)
    {
        return Err(DbError::Conflict(already_named(
            category_label(category),
            &stored,
        )));
    }
    create_tag(conn, category, &stored)?;
    tag_entry(conn, category, &stored)
}

fn rename_tag_filter(
    conn: &Connection,
    category: &str,
    name: &str,
    new_name: &str,
) -> Result<(), DbError> {
    let old = single_tag_name(name)?;
    let new = single_tag_name(new_name)?;
    let tags = list_tags(conn)?;
    let Some(tag) = tags
        .iter()
        .find(|tag| tag.category == category && tag.name == old)
    else {
        return Err(DbError::NotFound);
    };
    if old == new {
        return Ok(());
    }
    if tags
        .iter()
        .any(|other| other.category == category && other.name == new)
    {
        return Err(DbError::Conflict(rename_refused(
            category_label(category),
            &new,
        )));
    }
    let label = category_label(category);
    conn.execute(
        "UPDATE tags SET name = ?1 WHERE id = ?2",
        params![new, tag.id],
    )
    .map_err(|err| match DbError::from(err) {
        DbError::Conflict(_) => DbError::Conflict(rename_refused(label, &new)),
        other => other,
    })?;
    Ok(())
}

fn delete_tag_filter(conn: &Connection, category: &str, name: &str) -> Result<(), DbError> {
    let stored = single_tag_name(name)?;
    let tag = list_tags(conn)?
        .into_iter()
        .find(|tag| tag.category == category && tag.name == stored)
        .ok_or(DbError::NotFound)?;
    let used_by: i64 = conn.query_row(
        "SELECT COUNT(*) FROM miniature_tags WHERE tag_id = ?1",
        [tag.id],
        |row| row.get(0),
    )?;
    if used_by > 0 {
        return Err(DbError::Conflict(still_in_use(
            category_label(category),
            used_by,
        )));
    }
    conn.execute("DELETE FROM tags WHERE id = ?1", [tag.id])?;
    Ok(())
}

fn facet_group(conn: &Connection, category: &str, label: &str) -> Result<FilterGroup, DbError> {
    let names = facet_names(conn, category)?;
    let mut entries = Vec::with_capacity(names.len());
    for name in names {
        entries.push(facet_entry(conn, category, &name)?);
    }
    Ok(FilterGroup {
        category: category.to_string(),
        label: label.to_string(),
        entries,
    })
}

fn facet_names(conn: &Connection, category: &str) -> Result<Vec<String>, DbError> {
    let mut names = Vec::new();
    {
        let mut stmt = conn.prepare("SELECT name FROM facet_terms WHERE category = ?1")?;
        let rows = stmt.query_map([category], |row| row.get::<_, String>(0))?;
        for row in rows {
            push_one(&mut names, row?);
        }
    }
    let sql = match category {
        "size" => "SELECT size FROM miniatures",
        "game" => "SELECT game FROM miniatures WHERE game IS NOT NULL",
        _ => return Err(DbError::Invalid("category must be size or game".into())),
    };
    {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            push_one(&mut names, row?);
        }
    }
    names.sort_by(|left, right| {
        left.to_lowercase()
            .cmp(&right.to_lowercase())
            .then(left.cmp(right))
    });
    Ok(names)
}

fn facet_entry(conn: &Connection, category: &str, name: &str) -> Result<FilterEntry, DbError> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT name FROM facet_terms WHERE category = ?1 AND name = ?2 COLLATE NOCASE",
            params![category, name],
            |row| row.get(0),
        )
        .optional()?;
    Ok(plain_entry(
        stored.unwrap_or_else(|| name.to_string()),
        facet_used_by(conn, category, name)?,
    ))
}

fn facet_used_by(conn: &Connection, category: &str, name: &str) -> Result<i64, DbError> {
    let sql = match category {
        "size" => "SELECT COUNT(*) FROM miniatures WHERE size = ?1 COLLATE NOCASE",
        "game" => "SELECT COUNT(*) FROM miniatures WHERE game = ?1 COLLATE NOCASE",
        _ => return Err(DbError::Invalid("category must be size or game".into())),
    };
    Ok(conn.query_row(sql, [name], |row| row.get(0))?)
}

fn facet_known(conn: &Connection, category: &str, name: &str) -> Result<bool, DbError> {
    if facet_used_by(conn, category, name)? > 0 {
        return Ok(true);
    }
    let found: Option<i64> = conn
        .query_row(
            "SELECT id FROM facet_terms WHERE category = ?1 AND name = ?2 COLLATE NOCASE",
            params![category, name],
            |row| row.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

fn clean_facet_name(category: &str, name: &str) -> Result<String, DbError> {
    match category {
        "size" => required(name, "name", 32),
        "game" => required(name, "name", 120),
        _ => Err(DbError::Invalid("category must be size or game".into())),
    }
}

fn add_facet_filter(conn: &Connection, category: &str, name: &str) -> Result<FilterEntry, DbError> {
    let stored = clean_facet_name(category, name)?;
    if facet_known(conn, category, &stored)? {
        return Err(DbError::Conflict(already_named(category, &stored)));
    }
    conn.execute(
        "INSERT INTO facet_terms (category, name) VALUES (?1, ?2)",
        params![category, stored],
    )
    .map_err(|err| match DbError::from(err) {
        DbError::Conflict(_) => DbError::Conflict(already_named(category, &stored)),
        other => other,
    })?;
    facet_entry(conn, category, &stored)
}

fn rename_facet_filter(
    conn: &Connection,
    category: &str,
    name: &str,
    new_name: &str,
) -> Result<String, DbError> {
    let old = clean_facet_name(category, name)?;
    let new = clean_facet_name(category, new_name)?;
    if !facet_known(conn, category, &old)? {
        return Err(DbError::NotFound);
    }
    if !old.eq_ignore_ascii_case(&new) && facet_known(conn, category, &new)? {
        return Err(DbError::Conflict(rename_refused(category, &new)));
    }
    let tx = conn.unchecked_transaction()?;
    let mini_sql = match category {
        "size" => "UPDATE miniatures SET size = ?1 WHERE size = ?2 COLLATE NOCASE",
        "game" => "UPDATE miniatures SET game = ?1 WHERE game = ?2 COLLATE NOCASE",
        _ => return Err(DbError::Invalid("category must be size or game".into())),
    };
    tx.execute(mini_sql, params![new, old])?;
    tx.execute(
        "UPDATE facet_terms SET name = ?1 WHERE category = ?2 AND name = ?3 COLLATE NOCASE",
        params![new, category, old],
    )?;
    tx.commit()?;
    Ok(new)
}

fn delete_facet_filter(conn: &Connection, category: &str, name: &str) -> Result<(), DbError> {
    let stored = clean_facet_name(category, name)?;
    if !facet_known(conn, category, &stored)? {
        return Err(DbError::NotFound);
    }
    let used_by = facet_used_by(conn, category, &stored)?;
    if used_by > 0 {
        return Err(DbError::Conflict(still_in_use(category, used_by)));
    }
    let changed = conn.execute(
        "DELETE FROM facet_terms WHERE category = ?1 AND name = ?2 COLLATE NOCASE",
        params![category, stored],
    )?;
    if changed == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

fn location_group(conn: &Connection) -> Result<FilterGroup, DbError> {
    let locations = list_locations(conn)?;
    let mut entries = Vec::with_capacity(locations.len());
    for location in locations {
        entries.push(location_entry(conn, location.id)?);
    }
    Ok(FilterGroup {
        category: "location".into(),
        label: "Location".into(),
        entries,
    })
}

fn location_entry(conn: &Connection, id: i64) -> Result<FilterEntry, DbError> {
    let location = get_location(conn, id)?.ok_or(DbError::NotFound)?;
    let child_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM locations WHERE parent_id = ?1",
        [id],
        |row| row.get(0),
    )?;
    let used_by: i64 = conn.query_row(
        "SELECT COUNT(*) FROM miniatures WHERE location_id = ?1",
        [id],
        |row| row.get(0),
    )?;
    Ok(FilterEntry {
        name: location.name,
        used_by,
        id: Some(location.id),
        parent_id: location.parent_id,
        kind: Some(location.kind),
        path: Some(location.path),
        child_count: Some(child_count),
    })
}

fn sibling_name_taken(
    conn: &Connection,
    parent_id: Option<i64>,
    name: &str,
    except: Option<i64>,
) -> Result<bool, DbError> {
    let rows = load_location_rows(conn)?;
    Ok(rows
        .iter()
        .any(|row| row.parent_id == parent_id && row.name == name && except != Some(row.id)))
}

fn add_location_filter(
    conn: &Connection,
    name: &str,
    kind: &str,
    parent_id: Option<i64>,
) -> Result<FilterEntry, DbError> {
    let clean = clean_location_name(name)?;
    if sibling_name_taken(conn, parent_id, &clean, None)? {
        return Err(DbError::Conflict(format!(
            "a location named \"{clean}\" already exists in the same place."
        )));
    }
    let created = create_location(
        conn,
        NewLocation {
            name: clean,
            kind: kind.to_string(),
            parent_id,
        },
    )?;
    location_entry(conn, created.id)
}

fn rename_location_filter(
    conn: &Connection,
    id: i64,
    new_name: &str,
) -> Result<FilterEntry, DbError> {
    let location = get_location(conn, id)?.ok_or(DbError::NotFound)?;
    let clean = clean_location_name(new_name)?;
    if clean == location.name {
        return location_entry(conn, id);
    }
    if sibling_name_taken(conn, location.parent_id, &clean, Some(id))? {
        return Err(DbError::Conflict(format!(
            "a location named \"{clean}\" already exists in the same place. The rename was not applied."
        )));
    }
    update_location(
        conn,
        id,
        NewLocation {
            name: clean,
            kind: location.kind,
            parent_id: location.parent_id,
        },
    )?;
    location_entry(conn, id)
}

fn plain_entry(name: String, used_by: i64) -> FilterEntry {
    FilterEntry {
        name,
        used_by,
        id: None,
        parent_id: None,
        kind: None,
        path: None,
        child_count: None,
    }
}

pub fn insert_api_key(
    conn: &Connection,
    name: &str,
    key_hash: &str,
    scope: &str,
) -> Result<i64, DbError> {
    let name = required(name, "name", 80)?;
    conn.execute(
        "INSERT INTO api_keys (name, key_hash, scope) VALUES (?1, ?2, ?3)",
        params![name, key_hash, scope],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_api_keys(conn: &Connection) -> Result<Vec<ApiKeyRow>, DbError> {
    let mut stmt =
        conn.prepare("SELECT id, name, scope, created_at, revoked_at FROM api_keys ORDER BY id")?;
    let rows = stmt.query_map([], |row| {
        Ok(ApiKeyRow {
            id: row.get(0)?,
            name: row.get(1)?,
            scope: row.get(2)?,
            created_at: row.get(3)?,
            revoked_at: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
}

pub fn revoke_api_key(conn: &Connection, id: i64) -> Result<(), DbError> {
    let changed = conn.execute(
        "UPDATE api_keys SET revoked_at = datetime('now') WHERE id = ?1 AND revoked_at IS NULL",
        [id],
    )?;
    if changed == 1 {
        return Ok(());
    }
    let exists: Option<i64> = conn
        .query_row("SELECT id FROM api_keys WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    if exists.is_some() {
        return Ok(());
    }
    Err(DbError::NotFound)
}

pub fn active_scope(conn: &Connection, key_hash: &str) -> Result<Option<String>, DbError> {
    let scope = conn
        .query_row(
            "SELECT scope FROM api_keys WHERE key_hash = ?1 AND revoked_at IS NULL",
            [key_hash],
            |row| row.get(0),
        )
        .optional()?;
    Ok(scope)
}

/// Insert the example garage/office collection when the database has no locations yet.
pub fn seed_example(conn: &Connection) -> Result<bool, DbError> {
    let locations: i64 = conn.query_row("SELECT COUNT(*) FROM locations", [], |row| row.get(0))?;
    let miniatures: i64 =
        conn.query_row("SELECT COUNT(*) FROM miniatures", [], |row| row.get(0))?;
    if locations != 0 || miniatures != 0 {
        return Ok(false);
    }
    let tx = conn.unchecked_transaction()?;
    let garage = insert_location(
        &tx,
        &NewLocation {
            name: "Garage".into(),
            kind: "room".into(),
            parent_id: None,
        },
    )?;
    let cabinet = insert_location(
        &tx,
        &NewLocation {
            name: "Cabinet A".into(),
            kind: "cabinet".into(),
            parent_id: Some(garage),
        },
    )?;
    let drawer3 = insert_location(
        &tx,
        &NewLocation {
            name: "Drawer 3".into(),
            kind: "drawer".into(),
            parent_id: Some(cabinet),
        },
    )?;
    let box2 = insert_location(
        &tx,
        &NewLocation {
            name: "Box 2".into(),
            kind: "box".into(),
            parent_id: Some(drawer3),
        },
    )?;
    let drawer1 = insert_location(
        &tx,
        &NewLocation {
            name: "Drawer 1".into(),
            kind: "drawer".into(),
            parent_id: Some(cabinet),
        },
    )?;
    let tray = insert_location(
        &tx,
        &NewLocation {
            name: "Tray A".into(),
            kind: "tray".into(),
            parent_id: Some(drawer1),
        },
    )?;
    let office = insert_location(
        &tx,
        &NewLocation {
            name: "Office".into(),
            kind: "room".into(),
            parent_id: None,
        },
    )?;
    let shelf = insert_location(
        &tx,
        &NewLocation {
            name: "Shelf".into(),
            kind: "shelf".into(),
            parent_id: Some(office),
        },
    )?;
    let container = insert_location(
        &tx,
        &NewLocation {
            name: "Container 1".into(),
            kind: "container".into(),
            parent_id: Some(shelf),
        },
    )?;

    let samples = [
        sample(
            "Orc Warrior",
            "Sword and board.",
            4,
            "28mm",
            Some("Massive Darkness"),
            box2,
            &["orc"],
            &["barbarian"],
            &["melee"],
        ),
        sample(
            "Orc Archer",
            "",
            2,
            "28mm",
            Some("Massive Darkness"),
            tray,
            &["orc"],
            &["archer"],
            &["ranged"],
        ),
        sample(
            "Goblin Spearman",
            "Assorted sculpts.",
            8,
            "28mm",
            Some("Massive Darkness"),
            box2,
            &["goblin"],
            &["warrior"],
            &["melee"],
        ),
        sample(
            "Human Wizard",
            "Can stand in for any arcane caster.",
            1,
            "32mm",
            Some("Descent"),
            container,
            &["human"],
            &["wizard", "mage", "sorcerer"],
            &["magic"],
        ),
        sample(
            "Skeleton Warrior",
            "",
            5,
            "28mm",
            None,
            drawer1,
            &["undead"],
            &["warrior"],
            &["melee"],
        ),
        sample(
            "Elf Scout",
            "Bow and blades.",
            1,
            "32mm",
            Some("Descent"),
            container,
            &["elf"],
            &["ranger"],
            &["ranged", "melee"],
        ),
    ];
    for mini in samples {
        insert_miniature(&tx, &mini)?;
    }
    tx.commit()?;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn sample(
    name: &str,
    notes: &str,
    quantity: u32,
    size: &str,
    game: Option<&str>,
    location_id: i64,
    races: &[&str],
    roles: &[&str],
    combat: &[&str],
) -> NewMiniature {
    NewMiniature {
        name: name.into(),
        notes: notes.into(),
        quantity,
        size: size.into(),
        game: game.map(str::to_string),
        location_id,
        races: races.iter().map(|value| (*value).to_string()).collect(),
        roles: roles.iter().map(|value| (*value).to_string()).collect(),
        combat: combat.iter().map(|value| (*value).to_string()).collect(),
    }
}

fn insert_location(conn: &Connection, input: &NewLocation) -> Result<i64, DbError> {
    let name = clean_location_name(&input.name)?;
    let kind = required(&input.kind, "kind", 40)?;
    let rows = load_location_rows(conn)?;
    if let Some(parent) = input.parent_id {
        if !rows.iter().any(|row| row.id == parent) {
            return Err(DbError::Invalid("parent location does not exist".into()));
        }
    }
    conn.execute(
        "INSERT INTO locations (parent_id, name, kind) VALUES (?1, ?2, ?3)",
        params![input.parent_id, name, kind],
    )?;
    Ok(conn.last_insert_rowid())
}

fn insert_miniature(conn: &Connection, mini: &NewMiniature) -> Result<i64, DbError> {
    require_location(conn, mini.location_id)?;
    let mini = validate_miniature(mini.clone())?;
    conn.execute(
        "INSERT INTO miniatures (name, notes, quantity, size, game, location_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            mini.name,
            mini.notes,
            mini.quantity,
            mini.size,
            mini.game,
            mini.location_id
        ],
    )?;
    let id = conn.last_insert_rowid();
    replace_tags(conn, id, &mini)?;
    Ok(id)
}

fn require_location(conn: &Connection, id: i64) -> Result<(), DbError> {
    let exists: Option<i64> = conn
        .query_row("SELECT id FROM locations WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    if exists.is_none() {
        return Err(DbError::Invalid("location does not exist".into()));
    }
    Ok(())
}

fn replace_tags(conn: &Connection, miniature_id: i64, mini: &NewMiniature) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM miniature_tags WHERE miniature_id = ?1",
        [miniature_id],
    )?;
    for (category, names) in [
        ("race", &mini.races),
        ("role", &mini.roles),
        ("combat", &mini.combat),
    ] {
        for name in names {
            let tag_id = ensure_tag(conn, category, name)?;
            conn.execute(
                "INSERT INTO miniature_tags (miniature_id, tag_id) VALUES (?1, ?2)",
                params![miniature_id, tag_id],
            )?;
        }
    }
    Ok(())
}

fn ensure_tag(conn: &Connection, category: &str, name: &str) -> Result<i64, DbError> {
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM tags
             WHERE category = ?1 AND name = ?2 COLLATE NOCASE
             ORDER BY id LIMIT 1",
            params![category, name],
            |row| row.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO tags (category, name) VALUES (?1, ?2)
         ON CONFLICT(category, name) DO NOTHING",
        params![category, name],
    )?;
    conn.query_row(
        "SELECT id FROM tags WHERE category = ?1 AND name = ?2",
        params![category, name],
        |row| row.get(0),
    )
    .map_err(DbError::from)
}

fn load_location_rows(conn: &Connection) -> Result<Vec<LocRow>, DbError> {
    let mut stmt = conn.prepare("SELECT id, parent_id, name, kind FROM locations")?;
    let rows = stmt.query_map([], |row| {
        Ok(LocRow {
            id: row.get(0)?,
            parent_id: row.get(1)?,
            name: row.get(2)?,
            kind: row.get(3)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
}

fn path_of(rows: &[LocRow], id: i64) -> Option<String> {
    let mut names = Vec::new();
    let mut current = Some(id);
    let mut guard = 0;
    while let Some(node_id) = current {
        let node = rows.iter().find(|row| row.id == node_id)?;
        names.push(node.name.clone());
        current = node.parent_id;
        guard += 1;
        if guard > 1000 {
            return None;
        }
    }
    names.reverse();
    Some(names.join(" > "))
}

fn would_cycle(rows: &[LocRow], id: i64, new_parent: Option<i64>) -> bool {
    let mut current = new_parent;
    let mut guard = 0;
    while let Some(node_id) = current {
        if node_id == id {
            return true;
        }
        current = rows
            .iter()
            .find(|row| row.id == node_id)
            .and_then(|row| row.parent_id);
        guard += 1;
        if guard > 1000 {
            return true;
        }
    }
    false
}

fn load_miniatures(conn: &Connection, locations: &[Location]) -> Result<Vec<Miniature>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, notes, quantity, size, game, location_id, thumbnail_file
         FROM miniatures",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(MiniRow {
            id: row.get(0)?,
            name: row.get(1)?,
            notes: row.get(2)?,
            quantity: row.get(3)?,
            size: row.get(4)?,
            game: row.get(5)?,
            location_id: row.get(6)?,
            thumbnail_file: row.get(7)?,
        })
    })?;
    let rows = rows.collect::<Result<Vec<_>, _>>()?;
    let tags = load_tag_map(conn)?;
    let mut miniatures = Vec::with_capacity(rows.len());
    for row in rows {
        let owned = tags.get(&row.id).cloned().unwrap_or_default();
        let mut races = Vec::new();
        let mut roles = Vec::new();
        let mut combat = Vec::new();
        for (category, name) in owned {
            match category.as_str() {
                "race" => races.push(name),
                "role" => roles.push(name),
                "combat" => combat.push(name),
                _ => {}
            }
        }
        let location_path = locations
            .iter()
            .find(|location| location.id == row.location_id)
            .map(|location| location.path.clone())
            .unwrap_or_default();
        miniatures.push(Miniature {
            thumbnail_url: row
                .thumbnail_file
                .map(|_| format!("/api/v1/miniatures/{}/thumbnail", row.id)),
            id: row.id,
            name: row.name,
            notes: row.notes,
            quantity: u32::try_from(row.quantity).unwrap_or(0),
            size: row.size,
            game: row.game,
            location_id: row.location_id,
            location_path,
            races,
            roles,
            combat,
        });
    }
    Ok(miniatures)
}

fn load_tag_map(conn: &Connection) -> Result<HashMap<i64, Vec<(String, String)>>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT mt.miniature_id, t.category, t.name
         FROM miniature_tags mt
         JOIN tags t ON t.id = mt.tag_id
         ORDER BY t.name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut map: HashMap<i64, Vec<(String, String)>> = HashMap::new();
    for row in rows {
        let (id, category, name) = row?;
        map.entry(id).or_default().push((category, name));
    }
    Ok(map)
}

fn subtree_ids(locations: &[Location], roots: &[i64]) -> HashSet<i64> {
    let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
    for location in locations {
        if let Some(parent) = location.parent_id {
            children.entry(parent).or_default().push(location.id);
        }
    }
    let mut out = HashSet::new();
    for root in roots {
        walk(*root, &children, &mut out);
    }
    out
}

fn walk(id: i64, children: &HashMap<i64, Vec<i64>>, out: &mut HashSet<i64>) {
    if !out.insert(id) {
        return;
    }
    if let Some(kids) = children.get(&id) {
        for kid in kids {
            walk(*kid, children, out);
        }
    }
}

fn ci_any(have: &str, want: &[String]) -> bool {
    if want.iter().all(|value| value.trim().is_empty()) {
        return true;
    }
    want.iter()
        .any(|value| have.trim().eq_ignore_ascii_case(value.trim()))
}

fn game_any(have: Option<&str>, want: &[String]) -> bool {
    if want.iter().all(|value| value.trim().is_empty()) {
        return true;
    }
    let Some(have) = have else {
        return false;
    };
    want.iter()
        .any(|value| have.trim().eq_ignore_ascii_case(value.trim()))
}

fn push_one(values: &mut Vec<String>, value: String) {
    if !values
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&value))
    {
        values.push(value);
    }
}

fn required(value: &str, field: &str, max: usize) -> Result<String, DbError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(DbError::Invalid(format!("{field} is required")));
    }
    if trimmed.chars().count() > max {
        return Err(DbError::Invalid(format!("{field} is too long")));
    }
    Ok(trimmed.to_string())
}

fn optional_text(value: Option<&str>, field: &str, max: usize) -> Result<Option<String>, DbError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > max {
        return Err(DbError::Invalid(format!("{field} is too long")));
    }
    Ok(Some(trimmed.to_string()))
}

fn clean_location_name(value: &str) -> Result<String, DbError> {
    let name = required(value, "name", 120)?;
    if name.contains('>') {
        return Err(DbError::Invalid("name cannot contain '>'".into()));
    }
    Ok(name)
}

fn normalize_tags(values: &[String]) -> Result<Vec<String>, DbError> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        let name = value.trim();
        if name.is_empty() {
            continue;
        }
        if name.chars().count() > 64 {
            return Err(DbError::Invalid("a tag is too long".into()));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '/')
        {
            return Err(DbError::Invalid(format!(
                "tag \"{name}\" has unsupported characters"
            )));
        }
        if out
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(name))
        {
            continue;
        }
        out.push(name.to_string());
    }
    if out.len() > 20 {
        return Err(DbError::Invalid("too many tags in one category".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn
    }

    #[test]
    fn seed_paths_counts_and_search() {
        let conn = conn();
        assert!(seed_example(&conn).unwrap());
        assert!(!seed_example(&conn).unwrap());

        let locations = list_locations(&conn).unwrap();
        let box2 = locations
            .iter()
            .find(|location| location.name == "Box 2")
            .unwrap();
        assert_eq!(box2.path, "Garage > Cabinet A > Drawer 3 > Box 2");

        let report = location_report(&conn).unwrap();
        let by_name = |name: &str| report.iter().find(|row| row.name == name).unwrap();
        assert_eq!(by_name("Box 2").direct_quantity, 12);
        assert_eq!(by_name("Drawer 1").direct_quantity, 5);
        assert_eq!(by_name("Drawer 1").total_quantity, 7);
        assert_eq!(by_name("Cabinet A").direct_quantity, 0);
        assert_eq!(by_name("Cabinet A").total_quantity, 19);
        assert_eq!(by_name("Garage").total_quantity, 19);
        assert_eq!(by_name("Office").total_quantity, 2);

        let cabinet = locations
            .iter()
            .find(|location| location.name == "Cabinet A")
            .unwrap();
        let in_cabinet = list_miniatures(
            &conn,
            &MiniFilter {
                location_ids: vec![cabinet.id],
                ..MiniFilter::default()
            },
        )
        .unwrap();
        assert_eq!(in_cabinet.len(), 4);

        let archers = list_miniatures(
            &conn,
            &MiniFilter {
                races: vec!["orc".into()],
                combat: vec!["ranged".into()],
                ..MiniFilter::default()
            },
        )
        .unwrap();
        assert_eq!(archers.len(), 1);
        assert_eq!(archers[0].name, "Orc Archer");

        let wizards = list_miniatures(
            &conn,
            &MiniFilter {
                name: Some("wiz".into()),
                ..MiniFilter::default()
            },
        )
        .unwrap();
        assert_eq!(wizards.len(), 1);
        assert_eq!(wizards[0].roles, vec!["mage", "sorcerer", "wizard"]);
    }

    #[test]
    fn rejects_moving_a_location_inside_itself() {
        let conn = conn();
        let parent = create_location(
            &conn,
            NewLocation {
                name: "Garage".into(),
                kind: "room".into(),
                parent_id: None,
            },
        )
        .unwrap();
        let child = create_location(
            &conn,
            NewLocation {
                name: "Cabinet A".into(),
                kind: "cabinet".into(),
                parent_id: Some(parent.id),
            },
        )
        .unwrap();
        let err = update_location(
            &conn,
            parent.id,
            NewLocation {
                name: "Garage".into(),
                kind: "room".into(),
                parent_id: Some(child.id),
            },
        )
        .unwrap_err();
        assert!(matches!(err, DbError::Invalid(_)));
    }

    fn desk(conn: &Connection) -> i64 {
        create_location(
            conn,
            NewLocation {
                name: "Desk".into(),
                kind: "room".into(),
                parent_id: None,
            },
        )
        .unwrap()
        .id
    }

    fn mini(
        location_id: i64,
        races: Vec<String>,
        roles: Vec<String>,
        combat: Vec<String>,
    ) -> NewMiniature {
        NewMiniature {
            name: "Hero".into(),
            notes: String::new(),
            quantity: 1,
            size: "28mm".into(),
            game: None,
            location_id,
            races,
            roles,
            combat,
        }
    }

    #[test]
    fn tags_keep_the_case_that_was_entered() {
        let conn = conn();
        let location_id = desk(&conn);
        let created = create_miniature(
            &conn,
            mini(
                location_id,
                vec!["Orc".into()],
                vec!["Mage".into()],
                vec!["Magic and Melee".into()],
            ),
        )
        .unwrap();
        assert_eq!(created.races, vec!["Orc"]);
        assert_eq!(created.roles, vec!["Mage"]);
        assert_eq!(created.combat, vec!["Magic and Melee"]);

        let tag = create_tag(&conn, "race", "Goblin").unwrap();
        assert_eq!(tag.name, "Goblin");
    }

    #[test]
    fn tag_dedupe_is_case_insensitive_and_keeps_the_first_spelling() {
        let conn = conn();
        let location_id = desk(&conn);
        let first = create_miniature(
            &conn,
            mini(
                location_id,
                vec!["Orc".into(), " orc ".into(), "ORC".into()],
                vec!["Mage".into()],
                vec!["Ranged".into()],
            ),
        )
        .unwrap();
        assert_eq!(first.races, vec!["Orc"]);
        assert_eq!(first.combat, vec!["Ranged"]);

        let second = create_miniature(
            &conn,
            mini(
                location_id,
                vec!["orc".into()],
                vec!["mage".into()],
                vec!["ranged".into()],
            ),
        )
        .unwrap();
        assert_eq!(second.races, vec!["Orc"]);
        assert_eq!(second.roles, vec!["Mage"]);
        assert_eq!(second.combat, vec!["Ranged"]);

        let updated = update_miniature(
            &conn,
            first.id,
            validate_miniature(mini(
                location_id,
                vec!["orc".into(), "Elf".into()],
                vec!["Mage".into()],
                vec!["RANGED".into()],
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(updated.races.len(), 2);
        assert!(updated.races.iter().any(|race| race == "Orc"));
        assert!(updated.races.iter().any(|race| race == "Elf"));
        assert_eq!(updated.combat, vec!["Ranged"]);

        let found = list_miniatures(
            &conn,
            &MiniFilter {
                races: vec!["orc".into()],
                combat: vec!["RANGED".into()],
                ..MiniFilter::default()
            },
        )
        .unwrap();
        assert_eq!(found.len(), 2);

        let race_tags: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tags WHERE category = 'race' AND name = 'orc' COLLATE NOCASE",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(race_tags, 1);
        let all_races: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tags WHERE category = 'race'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(all_races, 2);
        let orc: String = conn
            .query_row(
                "SELECT name FROM tags WHERE category = 'race' AND name = 'orc' COLLATE NOCASE",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(orc, "Orc");

        let original = create_tag(&conn, "race", "Goblin").unwrap();
        let again = create_tag(&conn, "race", "goblin").unwrap();
        assert_eq!(again.id, original.id);
        assert_eq!(original.name, "Goblin");
        assert_eq!(again.name, "Goblin");
    }
}
