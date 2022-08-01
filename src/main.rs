use axum::{
    body::Body,
    extract::Path,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, get_service},
    Json, Router,
};
use errors::ApiError;
use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize};
use std::{env, path::PathBuf};
use std::{io, net::SocketAddr};
use tower::util::ServiceExt;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
mod errors;
use axum::http::Request;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "centarr=debug,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // `SpaRouter` is the easiest way to serve assets at a nested route like `/assets`
    // let app = Router::new()
    //     .route("/foo", get(|| async { "Hi from /foo" }))
    //     .merge(axum_extra::routing::SpaRouter::new("/assets", "."))
    //     .layer(TraceLayer::new_for_http());

    // for serving assets directly at the root you can use `tower_http::services::ServeDir`
    // as the fallback to a `Router`
    let app: _ = Router::new()
        .route("/shows", get(get_shows))
        .route("/shows/:showId", get(get_show))
        .route("/shows/:showId/episodes/:episodeId", get(get_episode))
        .route(
            "/shows/:showId/episodes/:episodeId/watch",
            get(get_episode_and_watch),
        )
        .fallback(get_service(ServeDir::new(".")).handle_error(handle_error))
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::debug!("listening on http://{}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

fn sonarr_url(path: &str) -> String {
    let url = format!("{}{}", env::var("SONARR_URL").unwrap(), path);
    return url;
}

fn sonarr_client(path: &str) -> RequestBuilder {
    let client = reqwest::Client::new();

    client
        .get(sonarr_url(path))
        .header("X-Api-Key", env::var("SONARR_API_KEY").unwrap())
}

#[derive(Serialize, Deserialize, Debug)]
struct Show {
    id: i32,
    title: String,
    images: Vec<ShowImage>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    episodes: Option<Vec<Episode>>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ShowImage {
    #[serde(rename = "coverType")]
    cover_type: String,
    url: String,
    #[serde(rename = "remoteUrl")]
    remote_url: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Episode {
    id: i32,
    #[serde(rename = "seriesId")]
    series_id: i32,
    #[serde(rename = "episodeFileId")]
    episode_file_id: i32,
    #[serde(rename = "seasonNumber")]
    season_number: i32,
    #[serde(rename = "episodeNumber")]
    episode_number: i32,
    title: String,
    #[serde(rename = "airDate")]
    air_date: String,
    #[serde(rename = "airDateUtc")]
    air_date_utc: String,
    overview: Option<String>,
    #[serde(rename = "episodeFile")]
    episode_file: Option<EpisodeFile>,
    #[serde(rename = "hasFile")]
    has_file: bool,
    monitored: bool,
    #[serde(rename = "absoluteEpisodeNumber")]
    absolute_episode_number: Option<i32>,
    #[serde(rename = "sceneAbsoluteEpisodeNumber")]
    scene_absolute_episode_number: Option<i32>,
    #[serde(rename = "sceneEpisodeNumber")]
    scene_episode_number: Option<i32>,
    #[serde(rename = "sceneSeasonNumber")]
    scene_season_number: Option<i32>,
    #[serde(rename = "unverifiedSceneNumbering")]
    unverified_scene_numbering: bool,
    #[serde(rename = "lastSearchTime")]
    last_search_time: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct EpisodeFile {
    id: i32,
    #[serde(rename = "seriesId")]
    series_id: i32,
    #[serde(rename = "seasonNumber")]
    season_number: i32,
    #[serde(rename = "relativePath")]
    relative_path: String,
    path: String,
    size: i32,
    #[serde(rename = "dateAdded")]
    date_added: String,
    // quality: Quality;
    // language: Language;
    // mediaInfo: MediaInfo;
    #[serde(rename = "originalFilePath")]
    original_file_path: String,
    #[serde(rename = "qualityCutoffNotMet")]
    quality_cutoff_not_met: bool,
    #[serde(rename = "sceneName")]
    scene_name: Option<String>,
}

async fn get_shows() -> Result<Json<Vec<Show>>, ApiError> {
    let body = sonarr_client("/series")
        .send()
        .await
        .map_err(|e| ApiError::empty(500, Some(e.to_string())))?
        .text()
        .await
        .map_err(|e| ApiError::empty(500, Some(e.to_string())))?;

    let shows = serde_json::from_str::<Vec<Show>>(&body).unwrap();

    return Ok(shows.into());
}

async fn get_show(Path(id): Path<i32>) -> Result<Json<Show>, ApiError> {
    let body = sonarr_client(format!("/series/{}", id).as_str())
        .send()
        .await
        .map_err(|e| ApiError::empty(500, Some(e.to_string())))?
        .text()
        .await
        .map_err(|e| ApiError::empty(500, Some(e.to_string())))?;

    let mut show = serde_json::from_str::<Show>(&body).unwrap();

    let body = sonarr_client(format!("/episode?seriesId={}", id).as_str())
        .send()
        .await
        .map_err(|e| ApiError::empty(500, Some(e.to_string())))?
        .text()
        .await
        .map_err(|e| ApiError::empty(500, Some(e.to_string())))?;

    let episodes = serde_json::from_str::<Vec<Episode>>(&body).unwrap();

    show.episodes = Some(episodes);

    return Ok(show.into());
}

async fn get_episode(Path(ids): Path<(i32, i32)>) -> Result<Json<Episode>, ApiError> {
    let body = sonarr_client(format!("/episode/{}?seriesId={}", ids.1, ids.0).as_str())
        .send()
        .await
        .map_err(|e| ApiError::empty(500, Some(e.to_string())))?
        .text()
        .await
        .map_err(|e| ApiError::empty(500, Some(e.to_string())))?;

    let episode = serde_json::from_str::<Episode>(&body).unwrap();

    return Ok(Json(episode));
}

async fn get_episode_and_watch(Path(ids): Path<(i32, i32)>) -> Result<impl IntoResponse, ApiError> {
    let body = sonarr_client(format!("/episode/{}?seriesId={}", ids.1, ids.0).as_str())
        .send()
        .await
        .map_err(|e| ApiError::empty(500, Some(e.to_string())))?
        .text()
        .await
        .map_err(|e| ApiError::empty(500, Some(e.to_string())))?;

    let episode = serde_json::from_str::<Episode>(&body).unwrap();

    if let Some(file) = episode.episode_file {
        let mut path = PathBuf::from(file.path.clone());

        if let Ok(prefix) = env::var("SONARR_DISK_PATH_PREFIX") {
            println!("here! {}", prefix);
            path = PathBuf::from(prefix).join(format!(".{}", file.path));
        }

        tracing::debug!("Opening file: {:?}", path);
        let svc = ServeFile::new(path);
        let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

        if res.status().is_success() {
            return Ok(res);
        }

        return Err(ApiError::new(500, "Couldn't find file on disk"));
    }

    Err(ApiError::new(400, "Episode not found"))
}

async fn handle_error(_err: io::Error) -> impl IntoResponse {
    (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong...")
}
