use crate::{
    utility::{load_latest_router_table, write_router_table},
    API, CODE, CODE_LENGTH, EXTERNEL_ID,
};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};
use tracing::info;
use url::Url;

#[derive(Deserialize, Serialize, Clone)]
pub struct Route {
    pub id: String,
    pub url: Url,
    pub params: HashMap<String, String>,
}

#[derive(Deserialize)]
pub struct RedirectParams {
    pub code: String,
}

#[derive(Clone)]
pub struct RouterState {
    router_url: Url,
    router_table_store: PathBuf,
    router_table: Arc<Mutex<HashMap<String, Route>>>,
    /// these are used for updating table to reduce impact on redirect service
    code_table: Arc<Mutex<HashMap<String, String>>>,
    router_table_admin: Arc<Mutex<HashMap<String, Route>>>,
}

#[derive(Debug)]
pub enum StateError {
    Unauthorized,
    IdNotFound,
    InvalidCode,
    StoreError(std::io::Error),
}

impl RouterState {
    /// load latest router table from disk, if any
    pub async fn init<P: AsRef<Path>>(
        router_url: &Url,
        router_table_store: P,
    ) -> Result<Self, StateError> {
        // create store if not exist
        tokio::fs::create_dir_all(router_table_store.as_ref())
            .await
            .map_err(|e| StateError::StoreError(e))?;
        // load router table
        let router_table = match load_latest_router_table(router_table_store.as_ref())
            .await
            .map_err(|e| StateError::StoreError(e))?
        {
            Some((time, table)) => {
                info!("router table loaded (time={time})");
                Arc::new(Mutex::new(table))
            }
            None => {
                info!("new router table created");
                Arc::new(Mutex::new(HashMap::new()))
            }
        };
        // clone to router table sync
        let router_table_admin = Arc::new(Mutex::new(router_table.lock().unwrap().clone()));
        let code_table = Arc::new(Mutex::new(
            router_table
                .lock()
                .unwrap()
                .iter()
                .map(|(code, route)| (route.id.to_string(), code.to_string()))
                .collect::<HashMap<_, _>>(),
        ));
        Ok(Self {
            router_url: router_url.clone(),
            router_table_store: router_table_store.as_ref().to_owned(),
            router_table,
            code_table,
            router_table_admin,
        })
    }

    // public API

    /// get the redirect url
    #[inline]
    pub fn redirect(&self, redirect_params: RedirectParams) -> Result<Url, StateError> {
        let lk = self.router_table.lock().unwrap();
        let route = lk
            .get(&redirect_params.code)
            .ok_or(StateError::IdNotFound)?;
        Ok(self.set_params(route, &redirect_params.code))
    }

    // admin APIs

    /// replace routing table
    pub async fn put_routing_table(&self, data: Vec<Route>) -> Result<(), StateError> {
        // use the sync table to reduce impect on redirect service
        let code_table_lk = self.code_table.clone();
        // create new router table
        let new_router_table = tokio::task::spawn_blocking(move || {
            let mut router_table_tmp = HashMap::new();
            let mut code_table = code_table_lk.lock().unwrap();
            for route in data {
                let code = Self::get_code(&mut code_table, &route.id).to_string();
                router_table_tmp.insert(code, route);
            }
            router_table_tmp
        })
        .await
        .map_err(|e| {
            StateError::StoreError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("background task error {e}"),
            ))
        })?;

        write_router_table(&new_router_table, &self.router_table_store)
            .await
            .map_err(|e| StateError::StoreError(e))?;

        // update router tables
        *self.router_table_admin.lock().unwrap() = new_router_table.clone();
        *self.router_table.lock().unwrap() = new_router_table;

        Ok(())
    }

    /// inserting routing table
    pub async fn patch_routing_table(&self, data: Vec<Route>) -> Result<(), StateError> {
        // use the sync table to reduce impect on redirect service
        let admin_table = self.router_table_admin.clone();
        let code_table_lk = self.code_table.clone();
        // create new router table
        let new_router_table = tokio::task::spawn_blocking(move || {
            let mut router_table_tmp = admin_table.lock().unwrap().clone();
            let mut code_table = code_table_lk.lock().unwrap();
            for route in data {
                let code = Self::get_code(&mut code_table, &route.id).to_string();
                router_table_tmp.insert(code, route);
            }
            router_table_tmp
        })
        .await
        .map_err(|e| {
            StateError::StoreError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("background task error {e}"),
            ))
        })?;

        write_router_table(&new_router_table, &self.router_table_store)
            .await
            .map_err(|e| StateError::StoreError(e))?;

        // update router tables
        *self.router_table_admin.lock().unwrap() = new_router_table.clone();
        *self.router_table.lock().unwrap() = new_router_table;

        Ok(())
    }

    /// get all links
    pub fn get_links(&self) -> Result<HashMap<String, Url>, StateError> {
        Ok(self
            .router_table_admin
            .lock()
            .unwrap()
            .iter()
            .map(|(code, route)| {
                (route.id.clone(), {
                    let mut url = self.router_url.clone();
                    url.set_path(API);
                    url.query_pairs_mut().append_pair(CODE, code).finish();
                    url
                })
            })
            .collect::<HashMap<_, _>>())
    }

    /// set params for redirected url.
    #[inline]
    fn set_params(&self, route: &Route, code: &str) -> Url {
        let mut url = route.url.clone();
        {
            let mut query = url.query_pairs_mut();
            route.params.iter().for_each(|(k, v)| {
                query.append_pair(k, v);
            });
            query.append_pair(EXTERNEL_ID, code);
            query.finish();
        }
        url
    }

    /// lookup or gen code.
    #[inline]
    fn get_code<'a>(code_table: &'a mut MutexGuard<HashMap<String, String>>, id: &str) -> &'a str {
        code_table.entry(id.to_string()).or_insert(
            rand::thread_rng()
                .sample_iter(Alphanumeric)
                .take(CODE_LENGTH)
                .map(char::from)
                .collect::<String>(),
        )
    }
}
