#![allow(deprecated)]
#![allow(private_interfaces)]
use tokio;
use std::env;
use axum::{
    routing::{get},
    Router,
};
use tower_http::{services::ServeDir};

mod handlers;

async fn get_backend() -> String {
    match env::var("BACKEND_URL") {
        Ok(s) => s,
        _ => "127.0.0.1:3001".to_string()
    }
}

#[tokio::main]
async fn main() {

    let serve_dir = ServeDir::new("static").not_found_service(ServeDir::new("./"));
    let app = Router::new()
        .route("/", get(handlers::index))
        .route("/community", get(handlers::community))
        .route("/followers", get(handlers::redirect_follower))
        .route("/analytics", get(handlers::analytics))
        .route("/followers/:user_id", get(handlers::followers))
		.route("/users/:id", get(handlers::profile_view))
        .route("/post", get(handlers::post))
        .route("/fortune", get(handlers::fortune))
        .route("/fortune/:id", get(handlers::fortune_profile))
        .nest_service("/static", serve_dir.clone())
        .fallback(handlers::handle_404);

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

