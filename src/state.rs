use crate::{config::Config, utility::*, API, CODE, CODE_LENGTH, EXTERNEL_ID};
use parking_lot::{Mutex, MutexGuard};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use tracing::info;
use url::Url;

#[derive(Deserialize, Serialize, Clone, Hash, PartialEq, Eq)]
pub struct Id(String);

#[derive(Deserialize, Serialize, Clone, Hash, PartialEq, Eq)]
pub struct Code(String);

#[derive(Deserialize, Serialize)]
pub struct Route {
    pub id: Id,
    pub url: Url,
}

#[derive(Deserialize)]
pub struct RedirectParams {
    pub code: Code,
}

#[derive(Clone)]
pub struct RouterState {
    pub router_url: Url,
    pub router_table_store: PathBuf,
    pub router_table: Arc<RwLock<HashMap<Code, Route>>>,
    pub code_table: Arc<Mutex<HashMap<Id, Code>>>,
}

#[derive(Debug)]
pub enum StateError {
    Unauthorized,
    InvalidCode,
    StoreError(std::io::Error),
}

impl RouterState {
    /// load latest router table from disk, if any
    pub async fn init(config: &Config) -> Result<Self, StateError> {
        // create store if not exist
        tokio::fs::create_dir_all(&config.storage_root)
            .await
            .map_err(StateError::StoreError)?;
        // load stored states
        let store = config.storage_root.clone();
        let (router_table, code_table) = tokio::task::spawn_blocking(move || {
            let router_table =
                match load_latest_router_table(&store).map_err(StateError::StoreError)? {
                    Some((time, table)) => {
                        info!("router table loaded (time={time})");
                        Arc::new(RwLock::new(table))
                    }
                    None => {
                        info!("new router table created");
                        Arc::new(RwLock::new(HashMap::new()))
                    }
                };
            let code_table = match load_latest_code_table(&store).map_err(StateError::StoreError)? {
                Some((time, table)) => {
                    info!("code table loaded (time={time})");
                    Arc::new(Mutex::new(table))
                }
                None => {
                    info!("new code table created");
                    Arc::new(Mutex::new(HashMap::new()))
                }
            };
            Ok((router_table, code_table))
        })
        .await
        .map_err(|e| {
            StateError::StoreError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("background task error {e}"),
            ))
        })??;
        Ok(Self {
            router_url: config.base_url.clone(),
            router_table_store: config.storage_root.clone(),
            router_table,
            code_table,
        })
    }

    // public API

    /// get the redirect url
    pub async fn redirect(&self, redirect_params: RedirectParams) -> Result<Url, StateError> {
        let mut url = self
            .router_table
            .read()
            .await
            .get(&redirect_params.code)
            .ok_or(StateError::InvalidCode)?
            .url
            .clone();
        {
            let mut query = url.query_pairs_mut();
            query.append_pair(EXTERNEL_ID, &redirect_params.code.0);
            query.finish();
        }
        Ok(url)
    }

    // admin APIs

    /// replace routing table
    pub async fn put_routing_table(&self, data: Vec<Route>) -> Result<(), StateError> {
        // use the sync table to reduce impect on redirect service
        let code_table_lk = self.code_table.clone();
        let store = self.router_table_store.clone();
        // create new router table
        let new_router_table = tokio::task::spawn_blocking(move || {
            let mut router_table_tmp = HashMap::with_capacity(data.len());
            let mut code_table = code_table_lk.lock();
            for route in data {
                let code = Self::get_code(&mut code_table, route.id.clone()).clone();
                router_table_tmp.insert(code, route);
            }
            // write tables
            write_router_table(&router_table_tmp, &store).map_err(StateError::StoreError)?;
            write_code_table(&code_table, &store).map_err(StateError::StoreError)?;
            Ok(router_table_tmp)
        })
        .await
        .map_err(|e| {
            StateError::StoreError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("background task error {e}"),
            ))
        })??;

        // update router tables
        *self.router_table.write().await = new_router_table;

        Ok(())
    }

    /// get all links
    pub async fn get_links(&self) -> Result<HashMap<Id, Url>, StateError> {
        Ok(self
            .router_table
            .read()
            .await
            .iter()
            .map(|(code, route)| {
                (route.id.clone(), {
                    let mut url = self.router_url.clone();
                    url.set_path(API);
                    url.query_pairs_mut().append_pair(CODE, &code.0).finish();
                    url
                })
            })
            .collect::<HashMap<_, _>>())
    }

    /// lookup or gen code.
    #[inline]
    fn get_code<'a>(code_table: &'a mut MutexGuard<HashMap<Id, Code>>, id: Id) -> &'a Code {
        code_table.entry(id).or_insert(Code(
            rand::thread_rng()
                .sample_iter(Alphanumeric)
                .take(CODE_LENGTH)
                .map(char::from)
                .collect::<String>(),
        ))
    }
}
