use crate::state::{RedirectParams, Route, RouterState, StateError};
use axum::{
    body::{Body, HttpBody},
    extract::{Query, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use tower_http::decompression::DecompressionBody;
use tracing::{error, info, warn};

pub async fn redirect(
    State(state): State<RouterState>,
    Query(redirect_params): Query<RedirectParams>,
) -> Response {
    match state.redirect(redirect_params).await {
        Ok(url) => {
            info!("redirect request to {url}");
            Redirect::to(url.as_str()).into_response()
        }
        Err(StateError::InvalidCode) => {
            warn!("request with invalid code");
            (StatusCode::NOT_FOUND, "invalid code").into_response()
        }
        Err(e) => {
            error!("fatal, unknown error when redirecting: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

pub async fn put_routing_table(
    State(state): State<RouterState>,
    req: Request<DecompressionBody<Body>>,
) -> Response {
    let data = match decode_request(req).await {
        Ok(data) => data,
        Err(rsp) => return rsp,
    };
    match state.put_routing_table(data).await {
        Ok(_) => {
            info!("put table success");
            (StatusCode::OK, "success").into_response()
        }
        Err(StateError::StoreError(e)) => {
            error!("storage error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "storage error").into_response()
        }
        Err(StateError::Busy) => {
            warn!("put table api busy");
            (StatusCode::TOO_MANY_REQUESTS, "busy, try again").into_response()
        }
        Err(e) => {
            error!("fatal, unknown error in put_routing_table: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "unknown error").into_response()
        }
    }
}

pub async fn get_links(State(state): State<RouterState>) -> Response {
    match state.get_links().await {
        Ok(links) => {
            info!("get links request");
            links
        }
        Err(StateError::Busy) => {
            warn!("get links api busy");
            (StatusCode::TOO_MANY_REQUESTS, "busy, try again").into_response()
        }
        Err(e) => {
            error!("fatal, unknown error in get_links: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "unknown error").into_response()
        }
    }
}

/// Decompress and parse json data
async fn decode_request(mut req: Request<DecompressionBody<Body>>) -> Result<Vec<Route>, Response> {
    let mut data = Vec::new();
    while let Some(chunk) = req.body_mut().data().await {
        match chunk {
            Ok(chunk) => data.extend_from_slice(&chunk[..]),
            Err(e) => {
                error!("error reading data: {e}");
                return Err((StatusCode::INTERNAL_SERVER_ERROR, "corrupt data").into_response());
            }
        }
    }
    serde_json::from_slice::<Vec<Route>>(&data).map_err(|e| {
        error!("json decode error: {e}");
        (StatusCode::BAD_REQUEST, "corrupt data").into_response()
    })
}
