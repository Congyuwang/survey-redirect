use crate::{
    utility::{load_latest_router_table, write_router_table},
    API, CODE, CODE_LENGTH, EXTERNEL_ID, ID,
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
    pub id: String,
    pub code: String,
}

#[derive(Clone)]
pub struct RouterState {
    router_url: Url,
    router_table_store: PathBuf,
    router_table: Arc<Mutex<HashMap<String, RouteVerify>>>,
    /// this is used for updating table to reduce impact on redirect service
    router_table_admin: Arc<Mutex<HashMap<String, RouteVerify>>>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct RouteVerify {
    code: String,
    route: Route,
}

#[derive(Debug)]
pub enum StateError {
    Unauthorized,
    IdNotFound,
    InvalidCode,
    StoreError(std::io::Error),
    InitError,
    JsonError,
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
        Ok(Self {
            router_url: router_url.clone(),
            router_table_store: router_table_store.as_ref().to_owned(),
            router_table,
            router_table_admin,
        })
    }

    // public API

    /// get the redirect url
    #[inline]
    pub fn redirect(&self, redirect_params: RedirectParams) -> Result<Url, StateError> {
        let lk = self.router_table.lock().unwrap();
        let route_verify = lk.get(&redirect_params.id).ok_or(StateError::IdNotFound)?;
        Self::verify_code(route_verify, &redirect_params)?;
        Ok(self.set_params(&route_verify.route.url, &route_verify))
    }

    // admin APIs

    /// replace routing table
    pub async fn put_routing_table(&self, data: Vec<Route>) -> Result<(), StateError> {
        // use the sync table to reduce impect on redirect service
        let router_table_lk = self.router_table_admin.clone();
        // create new router table
        let new_router_table = tokio::task::spawn_blocking(move || {
            let mut router_table_tmp = HashMap::new();
            let mut router_table = router_table_lk.lock().unwrap();
            for route in data {
                let code = Self::get_code(&mut router_table, &route.id).to_string();
                router_table_tmp.insert(route.id.clone(), RouteVerify { code, route });
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
    pub fn get_links(&self) -> Result<HashMap<String, String>, StateError> {
        let mut links = HashMap::new();
        for (id, route_verify) in self.router_table_admin.lock().unwrap().iter() {
            let mut url = self.router_url.clone();
            url.set_path(API);
            url.query_pairs_mut()
                .clear()
                .append_pair(ID, id)
                .append_pair(CODE, &route_verify.code);
            links.insert(id.clone(), url.into());
        }
        Ok(links)
    }

    /// verify code correctness.
    #[inline]
    fn verify_code(
        route_verify: &RouteVerify,
        redirect_params: &RedirectParams,
    ) -> Result<(), StateError> {
        if route_verify.code == redirect_params.code {
            Ok(())
        } else {
            Err(StateError::InvalidCode)
        }
    }

    /// set params for redirected url.
    #[inline]
    fn set_params(&self, url: &Url, route_verify: &RouteVerify) -> Url {
        let mut url = url.clone();
        {
            let mut query = url.query_pairs_mut();
            query.clear();
            route_verify.route.params.iter().for_each(|(k, v)| {
                query.append_pair(k, v);
            });
            query.append_pair(EXTERNEL_ID, &route_verify.code);
            query.finish();
        }
        url
    }

    /// lookup or gen code.
    #[inline]
    fn get_code(router_table: &mut MutexGuard<HashMap<String, RouteVerify>>, id: &str) -> String {
        router_table.get(id).map_or(
            rand::thread_rng()
                .sample_iter(Alphanumeric)
                .take(CODE_LENGTH)
                .map(char::from)
                .collect::<String>(),
            |route| route.code.clone(),
        )
    }
}
