use log::*;
use quick_xml::events::{BytesStart, Event};
use sqlx::migrate::MigrateDatabase;
use sqlx::types::JsonValue;
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek};
use std::path::PathBuf;
use time::{format_description, Date, OffsetDateTime};

lazy_static::lazy_static! {
    static ref HEALTHKIT_OFFSET_DATE_FORMAT_STR: &'static str =
        "[year]-[month]-[day] [hour]:[minute]:[second] [offset_hour sign:mandatory][offset_minute]";
    static ref HEALTHKIT_OFFSET_DATE_FORMAT : Vec<format_description::FormatItem<'static>> = format_description::parse(&HEALTHKIT_OFFSET_DATE_FORMAT_STR).expect("format parse");
    static ref HEALTHKIT_DATE_FORMAT_STR: &'static str =
        "[year]-[month]-[day]";
    static ref HEALTHKIT_DATE_FORMAT : Vec<format_description::FormatItem<'static>> = format_description::parse(&HEALTHKIT_DATE_FORMAT_STR).expect("format parse");

    // Static table names
    static ref WORKOUT_TABLE_NAME: &'static str = "Workout";
    static ref ACTIVITY_SUMMARY_TABLE_NAME: &'static str = "ActivitySummary";
}

/// A map of table names to a map of column names to SQL types
type HKTables = BTreeMap<String, BTreeMap<String, &'static str>>;
/// A list of (column name, value) tuples for insertion into a database table
type DatabaseRow = Vec<(String, DatabaseValue)>;

/// A typed value for insertion into the database
#[derive(Debug)]
enum DatabaseValue {
    Integer(i32),
    Real(f32),
    OffsetDateTime(OffsetDateTime),
    Date(Date),
    Text(String),
    Json(JsonValue),
}

/// Creates an SQLite database at the given URI containing all exported HealthKit data
pub async fn healthkit_to_sqlite(
    database_uri: &str,
    healthkit_zip_archive_path: &PathBuf,
) -> anyhow::Result<()> {
    let db = create_db(database_uri).await?;
    let exported_zip_archive_reader_0 = BufReader::new(File::open(healthkit_zip_archive_path)?);
    let exported_zip_archive_reader_1 = BufReader::new(File::open(healthkit_zip_archive_path)?);
    let mut data_archive = zip::ZipArchive::new(exported_zip_archive_reader_0)?;
    let mut routes_archive = zip::ZipArchive::new(exported_zip_archive_reader_1)?;
    // Pass 1: Create the database tables
    {
        let export_zip = data_archive.by_name("apple_health_export/export.xml")?;
        let reader = BufReader::with_capacity(export_zip.size() as usize, export_zip);
        let mut xml_reader = quick_xml::Reader::from_reader(reader);
        xml_reader.trim_text(true);

        let mut tx = db.begin().await?;
        sqlite_create_healthkit_tables(&mut tx, &mut xml_reader).await?;
        tx.commit().await?;
    }
    // Pass 2: Insert data into the database tables
    {
        let export_zip = data_archive.by_name("apple_health_export/export.xml")?;
        let reader = BufReader::with_capacity(export_zip.size() as usize, export_zip);
        let mut xml_reader = quick_xml::Reader::from_reader(reader);
        xml_reader.trim_text(true);

        let mut tx = db.begin().await?;
        sqlite_insert_healthkit_tables(&mut tx, &mut xml_reader, &mut routes_archive).await?;
        tx.commit().await?;
    }

    Ok(())
}

/// Converts an arbitrary string to a valid SQLite identifier
/// This currently isn't robust enough to handle all possible strings
/// But it's good enough for now. See https://stackoverflow.com/a/6701665
fn get_valid_sqlite_identifier(s: &str) -> String {
    format!("`{}`", s)
}

/// Derives and creates the SQLite tables from the exported HealthKit XML
async fn sqlite_create_healthkit_tables<R: BufRead>(
    tx: &mut Transaction<'_, Sqlite>,
    xml_reader: &mut quick_xml::Reader<R>,
) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    let mut tables: HKTables = HKTables::new();
    // Top-level parsing
    loop {
        match xml_reader.read_event_into(&mut buf) {
            Err(e) => panic!(
                "Error reading top-level HealthKit XML data at position {}: {:?}",
                xml_reader.buffer_position(),
                e
            ),
            Ok(Event::Start(e)) => {
                if let b"HealthData" = e.name().as_ref() {
                    debug!("HealthData: {:?}", e.attributes());
                    hk_create_health_data_tables(xml_reader, &mut tables, &mut buf).await?;
                }
            }
            Ok(Event::Eof) => break, // exits the loop when reaching end of file
            Ok(Event::Decl(_)) => continue, // continue loop on Decl event. We can use this to get encoding information (UTF8)
            Ok(Event::DocType(_)) => continue, // continue loop on DocType event. We can use this to get the top-level SCHEMA and HealthKit export version
            Ok(Event::End(_)) => continue,     // continue loop on End event
            Ok(Event::Empty(_)) => continue,   // continue loop on Empty event
            Ok(Event::Comment(_)) => continue, // continue loop on Comment event
            Ok(Event::CData(_)) => continue,   // continue loop on CData event
            Ok(Event::PI(_)) => continue,      // continue loop on PI event
            Ok(Event::Text(_)) => continue, // continue loop on Text event, don't care about text at the top level
        }
        buf.clear();
    }
    for (name, columns) in tables {
        let qs = format!(
            r#"CREATE TABLE IF NOT EXISTS `{}` ({});
        "#,
            name,
            columns
                .iter()
                .map(|(name, ty)| format!("{} {}", get_valid_sqlite_identifier(name), ty))
                .collect::<Vec<_>>()
                .join(", ")
        );
        sqlx::query(&qs).execute(&mut *tx).await?;
    }
    Ok(())
}

// Inserts the HealthKit data into the SQLite tables
async fn sqlite_insert_healthkit_tables<S: BufRead + Seek, R: BufRead>(
    tx: &mut Transaction<'_, Sqlite>,
    xml_reader: &mut quick_xml::Reader<R>,
    zip_archive: &mut zip::ZipArchive<S>,
) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    // Top-level parsing
    loop {
        match xml_reader.read_event_into(&mut buf) {
            Err(e) => panic!(
                "Error reading top-level HealthKit XML data at position {}: {:?}",
                xml_reader.buffer_position(),
                e
            ),
            Ok(Event::Start(e)) => {
                if let b"HealthData" = e.name().as_ref() {
                    debug!("HealthData: {:?}", e.attributes());
                    insert_hk_health_data_elements(tx, xml_reader, zip_archive).await?;
                }
            }
            Ok(Event::Eof) => break, // exits the loop when reaching end of file
            Ok(Event::Decl(_)) => continue, // continue loop on Decl event. We can use this to get encoding information (UTF8)
            Ok(Event::DocType(_)) => continue, // continue loop on DocType event. We can use this to get the top-level SCHEMA and HealthKit export version
            Ok(Event::End(_)) => continue,     // continue loop on End event
            Ok(Event::Empty(_)) => continue,   // continue loop on Empty event
            Ok(Event::Comment(_)) => continue, // continue loop on Comment event
            Ok(Event::CData(_)) => continue,   // continue loop on CData event
            Ok(Event::PI(_)) => continue,      // continue loop on PI event
            Ok(Event::Text(_)) => continue, // continue loop on Text event, don't care about text at the top level
        }
        buf.clear();
    }
    Ok(())
}

/// Derive the SQL type from a HealthKit value str
fn database_type_str_from_hk_value_str(value: &str) -> &'static str {
    lazy_static::lazy_static! {
        static ref INTEGER: &'static str = "INTEGER";
        static ref REAL: &'static str = "REAL";
        static ref DATE: &'static str = "DATE";
        static ref TEXT: &'static str = "TEXT";
    }
    if value.parse::<i32>().is_ok() {
        &INTEGER
    } else if value.parse::<f32>().is_ok() {
        &REAL
    } else if OffsetDateTime::parse(value, &HEALTHKIT_OFFSET_DATE_FORMAT).is_ok()
        || Date::parse(value, &HEALTHKIT_DATE_FORMAT).is_ok()
    {
        &DATE
    } else {
        &TEXT
    }
}

/// Returns a typed HKValue from a HealthKit value str
fn database_value_from_hk_value_str(value: &str) -> DatabaseValue {
    if let Ok(i) = value.parse::<i32>() {
        DatabaseValue::Integer(i)
    } else if let Ok(i) = value.parse::<f32>() {
        DatabaseValue::Real(i)
    } else if let Ok(i) = Date::parse(value, &HEALTHKIT_DATE_FORMAT) {
        DatabaseValue::Date(i)
    } else if let Ok(i) = OffsetDateTime::parse(value, &HEALTHKIT_OFFSET_DATE_FORMAT) {
        DatabaseValue::OffsetDateTime(i)
    } else {
        DatabaseValue::Text(value.to_string())
    }
}

fn hk_create_table_from_element<'a, R: BufRead>(
    reader: &mut quick_xml::Reader<R>,
    element: BytesStart<'a>,
    tables: &mut HKTables,
    table_name: &str,
) -> anyhow::Result<()> {
    if !tables.contains_key(table_name) {
        tables.insert(table_name.to_string(), Default::default());
    }
    let columns = tables.get_mut(table_name).expect("key must exist");
    for attribute in element.attributes() {
        let attribute = attribute?;
        let column_name_str = std::str::from_utf8(attribute.key.as_ref())?;
        if !columns.contains_key(column_name_str) {
            columns.insert(
                column_name_str.to_string(),
                database_type_str_from_hk_value_str(
                    attribute.decode_and_unescape_value(reader)?.as_ref(),
                ),
            );
        }
    }
    Ok(())
}

fn hk_table_append_metadata_entry_column<R: BufRead>(
    reader: &mut quick_xml::Reader<R>,
    element: BytesStart,
    tables: &mut HKTables,
    table_name: &str,
) -> anyhow::Result<()> {
    let columns = tables.get_mut(table_name).expect("cant fail");
    let mut key = Cow::Borrowed("");
    let mut value = Cow::Borrowed("");
    for attr_result in element.attributes() {
        let a = attr_result?;
        match a.key.as_ref() {
            b"key" => key = a.decode_and_unescape_value(reader)?,
            b"value" => value = a.decode_and_unescape_value(reader)?,
            _ => (),
        }
    }
    let column_name_str = key.as_ref();
    if !columns.contains_key(column_name_str) {
        columns.insert(
            // TODO
            format!("metadata_{}", column_name_str),
            database_type_str_from_hk_value_str(value.as_ref()),
        );
    }
    Ok(())
}

async fn hk_create_health_data_tables<R: BufRead>(
    reader: &mut quick_xml::Reader<R>,
    tables: &mut HKTables,
    buf: &mut Vec<u8>,
) -> anyhow::Result<()> {
    loop {
        match reader.read_event_into(buf)? {
            Event::Eof => break, // exits the loop when reaching end of file
            Event::Start(element) => match element.name().as_ref() {
                b"Workout" => {
                    let table_name = "Workout";
                    hk_create_table_from_element(reader, element, tables, table_name)?;
                    let mut inner_buf = Vec::new();
                    loop {
                        match reader.read_event_into(&mut inner_buf)? {
                            Event::Eof => break, // exits the loop when reaching end of file
                            Event::End(element) => {
                                if let b"Workout" = element.name().as_ref() {
                                    break;
                                }
                            }
                            Event::Empty(element) => match element.name().as_ref() {
                                b"MetadataEntry" => {
                                    hk_table_append_metadata_entry_column(
                                        reader, element, tables, table_name,
                                    )?;
                                }
                                b"WorkoutEvent" => {
                                    let columns = tables.get_mut(table_name).expect("cant fail");
                                    columns.insert("workoutEvents".to_string(), "JSON");
                                }
                                b"WorkoutStatistics" => {
                                    let columns = tables.get_mut(table_name).expect("cant fail");
                                    columns.insert("workoutStatistics".to_string(), "JSON");
                                }
                                other => {
                                    debug!(
                                        "Unhandled empty workout element: {:#?}",
                                        std::str::from_utf8(other)?
                                    );
                                }
                            },
                            Event::Start(element) => {
                                if b"WorkoutRoute" == element.name().as_ref() {
                                    let columns = tables.get_mut(table_name).expect("cant fail");
                                    columns.insert("geometry".to_string(), "JSON");
                                }
                            }
                            _ => continue,
                        }
                    }
                }
                b"Record" => {
                    let table_name = attribute_value_from_element(reader, &element, b"type")?;
                    hk_create_table_from_element(reader, element, tables, &table_name)?;
                    let mut inner_buf = Vec::new();
                    loop {
                        match reader.read_event_into(&mut inner_buf)? {
                            Event::Eof => break, // exits the loop when reaching end of file
                            Event::End(element) => {
                                if let b"Record" = element.name().as_ref() {
                                    break;
                                }
                            }
                            Event::Empty(element) => {
                                if b"MetadataEntry" == element.name().as_ref() {
                                    hk_table_append_metadata_entry_column(
                                        reader,
                                        element,
                                        tables,
                                        &table_name,
                                    )?;
                                }
                            }
                            Event::Start(_) => {}
                            _ => continue,
                        }
                    }
                }
                other => {
                    debug!(
                        "Unhandled HealthKit start element: {:#?}",
                        std::str::from_utf8(other)?
                    );
                }
            },
            Event::Empty(element) => match element.name().as_ref() {
                b"ExportDate" => {
                    // TODO
                    //debug!("ExportDate: {:?}", element.attributes());
                }
                b"Me" => {
                    // TODO
                    //debug!("Me: {:?}", element.attributes());
                }
                b"Record" => {
                    let table_name = attribute_value_from_element(reader, &element, b"type")?;
                    hk_create_table_from_element(reader, element, tables, &table_name)?;
                }
                b"ActivitySummary" => {
                    hk_create_table_from_element(
                        reader,
                        element,
                        tables,
                        &ACTIVITY_SUMMARY_TABLE_NAME,
                    )?;
                }
                _ => {}
            },
            Event::Decl(_) => continue, // continue loop on Decl event. We can use this to get encoding information (UTF8)
            Event::DocType(_) => continue, // continue loop on DocType event. We can use this to get the top-level SCHEMA and HealthKit export version
            Event::End(_) => continue,     // continue loop on End event
            Event::Comment(_) => continue, // continue loop on Comment event
            Event::CData(_) => continue,   // continue loop on CData event
            Event::PI(_) => continue,      // continue loop on PI event
            Event::Text(_) => continue, // continue loop on Text event, don't care about text at the top level
        }
        buf.clear();
    }
    Ok(())
}

async fn insert_hk_health_data_elements<S: BufRead + Seek, R: BufRead>(
    db: &mut Transaction<'_, Sqlite>,
    reader: &mut quick_xml::Reader<R>,
    zip_archive: &mut zip::ZipArchive<S>,
) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break, // exits the loop when reaching end of file
            Event::Start(element) => match element.name().as_ref() {
                b"Workout" => {
                    insert_hk_workout_element(db, reader, element, zip_archive).await?;
                }
                b"Record" => {
                    insert_hk_record_element(db, reader, element).await?;
                }
                other => {
                    debug!(
                        "Unhandled HealthKit element: {:#?}",
                        std::str::from_utf8(other)?
                    );
                }
            },
            Event::Decl(_) => continue, // continue loop on Decl event. We can use this to get encoding information (UTF8)
            Event::DocType(_) => continue, // continue loop on DocType event. We can use this to get the top-level SCHEMA and HealthKit export version
            Event::End(_) => continue,     // continue loop on End event
            Event::Empty(element) => match element.name().as_ref() {
                b"ExportDate" => {
                    debug!("ExportDate: {:?}", element.attributes());
                }
                b"Me" => {
                    debug!("Me: {:?}", element.attributes());
                }
                b"Record" => {
                    let table_name = attribute_value_from_element(reader, &element, b"type")?;
                    let row = database_row_from_element(reader, element)?;
                    insert_database_row(db, &table_name, row).await?;
                }
                b"ActivitySummary" => {
                    let row = database_row_from_element(reader, element)?;
                    insert_database_row(db, &ACTIVITY_SUMMARY_TABLE_NAME, row).await?;
                }
                _ => {}
            },
            Event::Comment(_) => continue, // continue loop on Comment event
            Event::CData(_) => continue,   // continue loop on CData event
            Event::PI(_) => continue,      // continue loop on PI event
            Event::Text(_) => continue, // continue loop on Text event, don't care about text at the top level
        }
        buf.clear();
    }
    Ok(())
}

fn database_row_from_element<R: BufRead>(
    reader: &mut quick_xml::Reader<R>,
    element: BytesStart,
) -> anyhow::Result<DatabaseRow> {
    let mut column = DatabaseRow::with_capacity(element.attributes().count());
    for attribute in element.attributes() {
        let attribute = attribute?;
        let column_name_str = std::str::from_utf8(attribute.key.as_ref())?;
        let column_value_str = attribute.decode_and_unescape_value(reader)?;
        column.push((
            column_name_str.to_string(),
            database_value_from_hk_value_str(&column_value_str),
        ));
    }
    Ok(column)
}

fn append_hk_metadata_entry_to_database_row<R: BufRead>(
    reader: &mut quick_xml::Reader<R>,
    element: BytesStart,
    mut record: DatabaseRow,
) -> anyhow::Result<DatabaseRow> {
    let mut key = Cow::Borrowed("");
    let mut value = Cow::Borrowed("");
    for attr_result in element.attributes() {
        let a = attr_result?;
        match a.key.as_ref() {
            b"key" => key = a.decode_and_unescape_value(reader)?,
            b"value" => value = a.decode_and_unescape_value(reader)?,
            _ => (),
        }
    }
    let column_name_str = key.as_ref();
    record.push((
        // TODO
        format!("metadata_{}", column_name_str),
        database_value_from_hk_value_str(value.as_ref()),
    ));
    Ok(record)
}

/// Converts a HealthKit XML element into a JSON object
fn json_value_from_hk_element<'a, R: BufRead>(
    reader: &mut quick_xml::Reader<R>,
    element: &BytesStart<'a>,
) -> anyhow::Result<JsonValue> {
    let mut json: BTreeMap<String, JsonValue> = Default::default();
    for attribute in element.attributes() {
        let attribute = attribute?;
        let attr_name_str = std::str::from_utf8(attribute.key.as_ref())?;
        let attr_value_str = attribute.decode_and_unescape_value(reader)?;
        let key = attr_name_str.to_string();
        let value = if let Ok(val) = attr_value_str.parse::<f32>() {
            val.into()
        } else {
            attr_value_str.into()
        };
        json.insert(key, value);
    }
    Ok(serde_json::to_value(json)?)
}

/// Returns the element attribute value as a string
fn attribute_value_from_element<'a, R: BufRead>(
    reader: &mut quick_xml::Reader<R>,
    element: &BytesStart<'a>,
    key: &[u8],
) -> anyhow::Result<String> {
    let mut table_name = Default::default();
    for attribute in element.attributes() {
        let attribute = attribute?;
        if key == attribute.key.as_ref() {
            table_name = attribute.decode_and_unescape_value(reader)?;
            break;
        }
    }
    if table_name.is_empty() {
        unreachable!("Workout element without workoutActivityType attribute");
    }
    Ok(table_name.to_string())
}

/// Inserts a single HealthKit Workout element into the Workout table
async fn insert_hk_workout_element<'a, S: BufRead + Seek, R: BufRead>(
    db: &mut Transaction<'_, Sqlite>,
    reader: &mut quick_xml::Reader<R>,
    element: BytesStart<'a>,
    zip_archive: &mut zip::ZipArchive<S>,
) -> anyhow::Result<()> {
    let mut row = database_row_from_element(reader, element)?;
    let mut buf = Vec::new();
    let mut workout_events = Vec::new();
    let mut workout_stats = BTreeMap::new();
    let mut workout_route = BTreeMap::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break, // exits the loop when reaching end of file
            Event::End(element) => {
                if b"Workout" == element.name().as_ref() {
                    break;
                }
            }
            Event::Empty(element) => match element.name().as_ref() {
                b"MetadataEntry" => {
                    row = append_hk_metadata_entry_to_database_row(reader, element, row)?;
                }
                b"WorkoutEvent" => {
                    workout_events.push(json_value_from_hk_element(reader, &element)?);
                }
                b"WorkoutStatistics" => {
                    let key = attribute_value_from_element(reader, &element, b"type")?;
                    let val = json_value_from_hk_element(reader, &element)?;
                    workout_stats.insert(key, val);
                }
                other => {
                    debug!(
                        "Unhandled empty workout element: {:#?}",
                        std::str::from_utf8(other)?
                    );
                }
            },
            // Handle the WorkoutRoute element
            Event::Start(element) => {
                if b"WorkoutRoute" == element.name().as_ref() {
                    let mut inner_buf = Vec::new();
                    loop {
                        match reader.read_event_into(&mut inner_buf)? {
                            Event::End(element) => {
                                if b"WorkoutRoute" == element.name().as_ref() {
                                    break;
                                }
                            }
                            Event::Empty(element) => {
                                if b"FileReference" == element.name().as_ref() {
                                    let mut path_value = Default::default();
                                    for attribute in element.attributes() {
                                        let attribute = attribute?;
                                        if b"path" == attribute.key.as_ref() {
                                            path_value =
                                                attribute.decode_and_unescape_value(reader)?;
                                            break;
                                        }
                                    }
                                    // Read the route gpx file at path_value
                                    debug!("Reading route gpx file: {}", path_value);
                                    let route_gpx_zip = zip_archive
                                        .by_name(&format!("apple_health_export{}", path_value))?;
                                    let route_reader = BufReader::with_capacity(
                                        route_gpx_zip.size() as usize,
                                        route_gpx_zip,
                                    );
                                    let mut route_xml =
                                        quick_xml::Reader::from_reader(route_reader);
                                    let mut coordinates = Vec::new();
                                    let mut route_buf = Vec::new();
                                    loop {
                                        match route_xml.read_event_into(&mut route_buf)? {
                                            // For now, we only care about extracting the lat/lon coordinates
                                            // from the trkpt elements
                                            Event::Start(element) => {
                                                if b"trkpt" == element.name().as_ref() {
                                                    let mut lat = Default::default();
                                                    let mut lon = Default::default();
                                                    for attribute in element.attributes() {
                                                        let attribute = attribute?;
                                                        if b"lat" == attribute.key.as_ref() {
                                                            lat = attribute
                                                                .decode_and_unescape_value(
                                                                    &route_xml,
                                                                )?;
                                                        } else if b"lon" == attribute.key.as_ref() {
                                                            lon = attribute
                                                                .decode_and_unescape_value(
                                                                    &route_xml,
                                                                )?;
                                                        }
                                                    }
                                                    coordinates.push(JsonValue::Array(vec![
                                                        lon.parse::<f32>()?.into(),
                                                        lat.parse::<f32>()?.into(),
                                                    ]));
                                                }
                                            }
                                            Event::Eof => break,
                                            _ => {}
                                        }
                                    }
                                    workout_route.insert(
                                        "type",
                                        JsonValue::String("LineString".to_string()),
                                    );
                                    workout_route
                                        .insert("coordinates", JsonValue::Array(coordinates));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => continue,
        }
        buf.clear();
    }
    row.push((
        "workoutEvents".to_string(),
        DatabaseValue::Json(serde_json::to_value(workout_events)?),
    ));
    row.push((
        "workoutStatistics".to_string(),
        DatabaseValue::Json(serde_json::to_value(workout_stats)?),
    ));
    row.push((
        "geometry".to_string(),
        DatabaseValue::Json(serde_json::to_value(workout_route)?),
    ));
    insert_database_row(db, &WORKOUT_TABLE_NAME, row).await?;
    Ok(())
}

/// Inserts a single HealthKit Record element into the appropriate database table
async fn insert_hk_record_element<'a, R: BufRead>(
    db: &mut Transaction<'_, Sqlite>,
    reader: &mut quick_xml::Reader<R>,
    element: BytesStart<'a>,
) -> anyhow::Result<()> {
    // The name of the record table comes from the type attribute
    let table_name = attribute_value_from_element(reader, &element, b"type")?;
    let mut row = database_row_from_element(reader, element)?;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::End(element) => {
                if b"Record" == element.name().as_ref() {
                    break;
                }
            }
            Event::Empty(element) => {
                if b"MetadataEntry" == element.name().as_ref() {
                    row = append_hk_metadata_entry_to_database_row(reader, element, row)?;
                }
            }
            Event::Start(_) => {}
            _ => continue,
        }
        buf.clear();
    }
    insert_database_row(db, &table_name, row).await?;
    Ok(())
}

/// Inserts a single database row into the specified table
async fn insert_database_row(
    db: &mut Transaction<'_, Sqlite>,
    table_name: &str,
    row: DatabaseRow,
) -> anyhow::Result<()> {
    let qs = format!(
        r#"INSERT INTO {} ({}) VALUES ({})"#,
        table_name,
        row.iter()
            .map(|(name, _)| get_valid_sqlite_identifier(name))
            .collect::<Vec<_>>()
            .join(", "),
        row.iter()
            .map(|(_, _)| "?")
            .collect::<Vec<&str>>()
            .join(", ")
    );
    let mut query = sqlx::query(&qs);
    for (_, value) in row {
        match value {
            DatabaseValue::Integer(i) => query = query.bind(i),
            DatabaseValue::Real(i) => query = query.bind(i),
            DatabaseValue::OffsetDateTime(i) => query = query.bind(i),
            DatabaseValue::Date(i) => query = query.bind(i),
            DatabaseValue::Text(i) => query = query.bind(i),
            DatabaseValue::Json(i) => query = query.bind(i),
        }
    }
    query.execute(&mut *db).await?;
    Ok(())
}

async fn create_db(db_url: &str) -> anyhow::Result<SqlitePool> {
    // Create the database
    if !sqlx::Sqlite::database_exists(db_url).await? {
        sqlx::Sqlite::create_database(db_url).await?;
    }

    // Connect to the database
    let db = SqlitePool::connect(db_url).await?;
    // Run migrations
    sqlx::migrate!().run(&db).await?;
    Ok(db)
}
