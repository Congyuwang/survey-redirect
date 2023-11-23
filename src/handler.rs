use crate::state::{RedirectParams, Route, RouterState, StateError};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    Json,
};

use tracing::{error, info, warn};

pub async fn redirect(
    State(state): State<RouterState>,
    Query(redirect_params): Query<RedirectParams>,
) -> Response {
    match state.redirect(redirect_params) {
        Ok(url) => {
            info!("redirect request to {url}");
            Redirect::to(url.as_str()).into_response()
        }
        Err(StateError::IdNotFound) => {
            warn!("request with invalid id");
            (StatusCode::NOT_FOUND, "invalid uri").into_response()
        }
        Err(StateError::InvalidCode) => {
            warn!("request with invalid code");
            (StatusCode::NOT_FOUND, "invalid uri").into_response()
        }
        Err(e) => {
            error!("fatal, unknown error when redirecting: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

pub async fn put_routing_table(
    State(state): State<RouterState>,
    Json(data): Json<Vec<Route>>,
) -> Response {
    match state.put_routing_table(data).await {
        Ok(_) => {
            info!("put table success");
            (StatusCode::OK, "success").into_response()
        }
        Err(StateError::StoreError(e)) => {
            error!("storage error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("storage error: {e}"),
            )
                .into_response()
        }
        Err(e) => {
            error!("fatal, unknown error in put_routing_table: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("fatal, unknown error in put_routing_table: {:?}", e),
            )
                .into_response()
        }
    }
}

pub async fn get_links(State(state): State<RouterState>) -> Response {
    match state.get_links() {
        Ok(links) => {
            info!("got link request");
            Json(links).into_response()
        }
        Err(e) => {
            error!("fatal, unknown error in get_links: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("fatal, unknown error in get_links: {:?}", e),
            )
                .into_response()
        }
    }
}
