use crate::{state::Route, SERVER_CONFIG};
use axum::{
    http::{header::AUTHORIZATION, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use chrono::{DateTime, FixedOffset};
use serde::{de::DeserializeOwned, Serialize};
use std::{collections::HashMap, path::Path};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const JSON_EXT: &str = "json";

/// auth middleware
pub async fn auth<B>(req: Request<B>, next: Next<B>) -> Result<Response, StatusCode> {
    let auth_header = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|header| header.to_str().ok());

    let auth_header = auth_header.ok_or(StatusCode::UNAUTHORIZED)?;

    if auth_header == &SERVER_CONFIG.admin_token {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// write new code table & redirect table to a file path
pub async fn write_router_table<P: AsRef<Path>>(
    router_table: &HashMap<String, Route>,
    router_directory: P,
) -> Result<(), std::io::Error> {
    let timestamp = chrono::Local::now().to_rfc3339();

    let route_file = {
        let mut dst = router_directory.as_ref().to_owned();
        dst.push(format!("{}.{}", timestamp, JSON_EXT));
        dst
    };

    write_data(route_file, router_table).await?;

    Ok(())
}

/// load latest code table & router table
///
/// It searches for a timestamp where *BOTH* table exists.
pub async fn load_latest_router_table<P: AsRef<Path>>(
    router_directory: P,
) -> Result<Option<(DateTime<FixedOffset>, HashMap<String, Route>)>, std::io::Error> {
    let mut latest = None;
    let mut dir = tokio::fs::read_dir(router_directory).await?;
    while let Some(entry) = dir.next_entry().await? {
        // skip folder and symlinks
        if !entry.file_type().await?.is_file() {
            continue;
        }

        // extract time from time.json
        let path = entry.path();
        let this_time = {
            if Some(JSON_EXT) == path.extension().map(|ext| ext.to_str()).flatten() {
                if let Some(Ok(this_time)) = path
                    .file_stem()
                    .map(|name| name.to_str())
                    .flatten()
                    .map(|name| chrono::DateTime::parse_from_rfc3339(name))
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

    // load data
    if let Some((time, entry)) = latest {
        Ok(Some((time, load_data(entry.path()).await?)))
    } else {
        Ok(None)
    }
}

/// load data
async fn load_data<P: AsRef<Path> + Send + 'static, T: DeserializeOwned>(
    file_path: P,
) -> Result<T, std::io::Error> {
    let mut buf = Vec::new();
    tokio::fs::File::open(file_path)
        .await?
        .read_to_end(&mut buf)
        .await?;
    serde_json::from_slice::<T>(&buf).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("json deserialization error: {e}"),
        )
    })
}

/// write data
async fn write_data<P: AsRef<Path> + Send + 'static, T: Serialize>(
    file_path: P,
    data: &T,
) -> Result<(), std::io::Error> {
    // serialize data
    let data = serde_json::to_string(data).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("json serialization error: {e}"),
        )
    })?;

    // create a temp file
    let temp = asyncify(|| tempfile::NamedTempFile::new()).await?;

    // write to temp file
    let (temp_file, temp_path) = temp.into_parts();
    let mut temp_file = tokio::fs::File::from_std(temp_file);
    temp_file.write_all(data.as_ref()).await?;
    let temp = tempfile::NamedTempFile::from_parts(temp_file, temp_path);

    // persist file
    asyncify(|| temp.persist(file_path).map_err(|e| e.error)).await?;
    Ok(())
}

/// spawn blocking io
async fn asyncify<F, T>(f: F) -> std::io::Result<T>
where
    F: FnOnce() -> std::io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(res) => res,
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "background task failed",
        )),
    }
}
