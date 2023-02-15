use std::fs;
use axum::body::{Body, Empty, Full};
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, Request};
use axum::middleware::Next;
use axum::{
    body,
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use clap::Parser;
use std::net::SocketAddr;
use std::sync::{Arc};
use time::{macros::format_description, UtcOffset};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, fmt::time::OffsetTime, fmt};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
struct AccessLog {
    uri: String,
    method: String,
    req_body: String,
    status_code: u16
}

#[derive(Parser)]
struct AppSetting {
    #[arg(short, long)]
    static_dir: String,

    #[arg(short, long, default_value_t = 3000)]
    port: u16
}

async fn static_path_handler(Path(path): Path<String>, State(app_setting): State<Arc<AppSetting>>) -> impl IntoResponse {
    let mut path = path.trim_start_matches('/');
    if path.is_empty() {
        path = "index.html";
    }

    let mime_type = mime_guess::from_path(path).first_or_text_plain();
    let static_dir = std::path::Path::new(&app_setting.static_dir);
    let file_path = static_dir.join(path);

    match file_path.exists() {
        false => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(body::boxed(Empty::new()))
            .unwrap(),
        true => {
            let file_content = fs::read(file_path).unwrap();
            Response::builder()
                .status(StatusCode::OK)
                .header(
                    header::CONTENT_TYPE,
                    HeaderValue::from_str(mime_type.as_ref()).unwrap(),
                )
                .body(body::boxed(Full::from(file_content)))
                .unwrap()
        }
    }
}

async fn extract_req_res_info(req: Request<Body>, next: Next<Body>) -> impl IntoResponse {
    let (parts, req_body) = req.into_parts();
    let uri = parts.uri.to_string();
    let method = parts.method.to_string();
    let req_body_bytes = hyper::body::to_bytes(req_body).await.unwrap();
    let req_body = std::str::from_utf8(req_body_bytes.as_ref()).unwrap().to_string();

    let mut access_log = AccessLog {
        uri,
        method,
        req_body,
        status_code: StatusCode::OK.as_u16()
    };
    let req = Request::from_parts(parts, Body::from(req_body_bytes));
    let res = next.run(req).await;
    let status_code = res.status().clone();
    access_log.status_code = status_code.as_u16();
    tracing::debug!(
            "{}", serde_json::to_string(&access_log).unwrap()
    );
    res
}

#[tokio::main]
async fn main() {
    let local_time = OffsetTime::new(
        UtcOffset::from_hms(8, 0, 0).unwrap(),
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]"),
    );

    let fmt_layer = fmt::layer()
        .with_timer(local_time);
        // .pretty();

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "static_server=debug,tower_http=debug".into()),
        )
        .init();

    let app_setting = AppSetting::parse();
    let port = app_setting.port;

    let state = Arc::new(app_setting);

    let app = Router::new()
        .route("/*path", get(static_path_handler))
        .layer(middleware::from_fn(extract_req_res_info))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::debug!("listening on {}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
