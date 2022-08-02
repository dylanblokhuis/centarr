use axum::{
    body::Body,
    extract::Path,
    http::{HeaderMap, Response},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use errors::ApiError;

use regex::Regex;
use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::{env, os::unix::prelude::FileExt, path::PathBuf};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
mod errors;

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

static CHUNK_SIZE: u64 = 300_000_0;

async fn get_episode_and_watch(
    Path(ids): Path<(i32, i32)>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
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
            path = PathBuf::from(prefix).join(format!(".{}", file.path));
        }

        let file = std::fs::File::open(path).unwrap();
        let metadata = file.metadata().unwrap();

        let range = headers.get(axum::http::header::RANGE);

        let mut start_index = 0;
        let mut end_index: u64 = 0;

        if let Some(range) = range {
            let re = Regex::new(r"bytes=(\d+)-(\d+)?").unwrap();
            let captures = re.captures(range.to_str().unwrap()).unwrap();
            let start = captures.get(1).unwrap().as_str();
            start_index = start.parse::<u64>().unwrap();

            if let Some(end) = captures.get(2) {
                end_index = end.as_str().parse::<u64>().unwrap();
            }
        }

        if start_index == 0 && end_index == 0 {
            end_index = std::cmp::min(metadata.len(), start_index + CHUNK_SIZE);
        }

        if start_index != 0 && end_index == 0 {
            end_index = std::cmp::min(metadata.len(), start_index + CHUNK_SIZE);
        }

        let read_amount = end_index - start_index;
        let mut buf: Vec<u8> = vec![0; read_amount as usize];

        let bytes_read = file.read_at(&mut buf, start_index).unwrap();
        println!("bytes_read: {:?}", bytes_read);

        let res = Response::builder()
            .status(206)
            .header("Accept-Ranges", "Bytes")
            .header("Content-Type", "video/webm")
            .header(
                "Content-Range",
                format!("bytes {}-{}/{}", start_index, end_index, metadata.len()),
            )
            .body(Body::from(buf))
            .unwrap();

        return Ok(res);

        // return Err(ApiError::new(500, "Couldn't find file on disk"));
    }

    Err(ApiError::new(400, "Episode not found"))
}
