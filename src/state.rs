use crate::{config::Config, utility::*, API, CODE, CODE_LENGTH, EXTERNEL_ID};
use axum::{
    response::{IntoResponse, Response},
    Json,
};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, MutexGuard, RwLock};
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
    pub router_table: Arc<RwLock<HashMap<Code, Url>>>,
    pub code_table: Arc<Mutex<HashMap<Id, Code>>>,
}

#[derive(Debug)]
pub enum StateError {
    Unauthorized,
    InvalidCode,
    StoreError(std::io::Error),
    Busy,
}

impl RouterState {
    pub fn init(config: &Config) -> Result<Self, StateError> {
        // create store if not exist
        std::fs::create_dir_all(&config.storage_root).map_err(StateError::StoreError)?;
        // load stored states
        let store = config.storage_root.clone();
        let router_table = match load_latest_router_table(&store).map_err(StateError::StoreError)? {
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
            Some(table) => {
                info!("code table loaded");
                Arc::new(Mutex::new(table))
            }
            None => {
                info!("new code table created");
                Arc::new(Mutex::new(HashMap::new()))
            }
        };
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
    ///
    /// returns `Err(Busy)` if cannot acquire a lock of code_table.
    pub async fn put_routing_table(&self, data: Vec<Route>) -> Result<(), StateError> {
        let new_router_table = {
            let mut code_table_lk = self.code_table.try_lock().or(Err(StateError::Busy))?;
            // at most one block_in_place call
            tokio::task::block_in_place(|| {
                let mut tmp = HashMap::with_capacity(data.len());
                for route in data {
                    let code = Self::get_code(&mut code_table_lk, route.id).clone();
                    tmp.insert(code, route.url);
                }
                // write tables
                write_code_table(&code_table_lk, &self.router_table_store)
                    .map_err(StateError::StoreError)?;
                write_router_table(&tmp, &self.router_table_store)
                    .map_err(StateError::StoreError)?;
                Ok::<_, StateError>(tmp)
            })?
        };
        *self.router_table.write().await = new_router_table;
        Ok(())
    }

    /// partially update routing table
    ///
    /// returns `Err(Busy)` if cannot acquire a lock of code_table.
    pub async fn patch_routing_table(&self, data: Vec<Route>) -> Result<(), StateError> {
        let new_router_table = {
            let mut code_table_lk = self.code_table.try_lock().map_err(|_| StateError::Busy)?;
            let mut tmp = self.router_table.read().await.clone();
            // at most one block_in_place call
            tokio::task::block_in_place(|| {
                for route in data {
                    let code = Self::get_code(&mut code_table_lk, route.id).clone();
                    tmp.insert(code, route.url);
                }
                // write tables
                write_code_table(&code_table_lk, &self.router_table_store)
                    .map_err(StateError::StoreError)?;
                write_router_table(&tmp, &self.router_table_store)
                    .map_err(StateError::StoreError)?;
                Ok::<_, StateError>(tmp)
            })?
        };
        *self.router_table.write().await = new_router_table;
        Ok(())
    }

    /// get all links
    ///
    /// returns `Err(Busy)` if cannot acquire a lock of code_table.
    pub async fn get_links(&self) -> Result<Response, StateError> {
        let router_table_lk = self.router_table.read().await;
        let code_table_lk = self.code_table.try_lock().map_err(|_| StateError::Busy)?;
        let mut links: HashMap<&Id, Url> = HashMap::with_capacity(router_table_lk.len());
        for (id, code) in code_table_lk.iter() {
            if router_table_lk.contains_key(code) {
                let mut url = self.router_url.clone();
                url.set_path(API);
                url.query_pairs_mut().append_pair(CODE, &code.0).finish();
                links.insert(id, url);
            }
        }
        Ok(Json(links).into_response())
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
