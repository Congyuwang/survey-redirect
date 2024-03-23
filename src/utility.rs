//! All functions in this file are blocking functions!
//! Must call within `spawn_blocking`.
use crate::state::{Code, Uid};
use chrono::{DateTime, FixedOffset};
use serde::{de::DeserializeOwned, Serialize};
use std::fs::DirEntry;
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
};
use url::Url;

const JSON_EXT: &str = "json";
const CODE_TABLE: &str = "code";

type TimeStamp = DateTime<FixedOffset>;

pub fn write_router_table<P: AsRef<Path>>(
    router_table: &HashMap<Code, Url>,
    router_directory: P,
) -> std::io::Result<()> {
    write_data_with_timestamp_ext(router_table, router_directory, JSON_EXT)
}

pub fn write_code_table<P: AsRef<Path>>(
    code_table: &HashMap<Uid, Code>,
    router_directory: P,
) -> std::io::Result<()> {
    let file = {
        let mut dst = router_directory.as_ref().to_owned();
        dst.push(CODE_TABLE);
        dst
    };
    write_data(file, code_table)
}

fn write_data_with_timestamp_ext<P: AsRef<Path>, T: Serialize>(
    data: &T,
    dir: P,
    ext: &str,
) -> std::io::Result<()> {
    let timestamp = chrono::Local::now().to_rfc3339();
    let file = {
        let mut dst = dir.as_ref().to_owned();
        dst.push(format!("{}.{}", timestamp, ext));
        dst
    };
    write_data(file, data)
}

fn write_data<P: AsRef<Path> + Send + 'static, T: Serialize>(
    file_path: P,
    data: &T,
) -> std::io::Result<()> {
    // serialize data
    let data = serde_json::to_string(data).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("json serialization error: {e}"),
        )
    })?;
    // create a temp file
    let temp = tempfile::NamedTempFile::new()?;
    // write to temp file
    let (mut temp_file, temp_path) = temp.into_parts();
    temp_file.write_all(data.as_ref())?;
    let temp = tempfile::NamedTempFile::from_parts(temp_file, temp_path);
    // persist file
    temp.persist(file_path).map_err(|e| e.error)?;
    Ok(())
}

//
// Load-related functions are async.
//

pub fn load_latest_router_table<P: AsRef<Path>>(
    router_directory: P,
) -> std::io::Result<Option<(TimeStamp, HashMap<Code, Url>)>> {
    let latest = get_latest_file_with_ext(router_directory, JSON_EXT)?;
    // load data
    if let Some((time, entry)) = latest {
        Ok(Some((time, load_data(entry.path())?)))
    } else {
        Ok(None)
    }
}

pub fn load_latest_code_table<P: AsRef<Path>>(
    router_directory: P,
) -> std::io::Result<Option<HashMap<Uid, Code>>> {
    let latest = {
        let mut dst = router_directory.as_ref().to_owned();
        dst.push(CODE_TABLE);
        dst
    };
    // load data
    if latest.is_file() {
        Ok(Some(load_data(latest)?))
    } else {
        Ok(None)
    }
}

/// get latest file with extension
fn get_latest_file_with_ext<P: AsRef<Path>>(
    dir: P,
    ext: &str,
) -> std::io::Result<Option<(TimeStamp, DirEntry)>> {
    let mut latest = None;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        // skip folder and symlinks
        if !entry.file_type()?.is_file() {
            continue;
        }

        // extract time from time.json
        let this_time = {
            let path = entry.path();
            if Some(ext) == path.extension().and_then(|ext| ext.to_str()) {
                if let Some(Ok(this_time)) = path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .map(chrono::DateTime::parse_from_rfc3339)
                {
                    this_time
                } else {
                    continue;
                }
            } else {
                continue;
            }
        };

        // update latest entry
        if let Some((time, e)) = latest.as_mut() {
            if *time < this_time {
                *time = this_time;
                *e = entry;
            }
        } else {
            latest = Some((this_time, entry));
        }
    }
    Ok(latest)
}

/// load data
fn load_data<P: AsRef<Path> + Send + 'static, T: DeserializeOwned>(
    file_path: P,
) -> std::io::Result<T> {
    let mut buf = Vec::new();
    std::fs::File::open(file_path)?.read_to_end(&mut buf)?;
    serde_json::from_slice::<T>(&buf).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("json deserialization error: {e}"),
        )
    })
}
