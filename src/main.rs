use axum::{extract::Path, http::HeaderMap, routing::get, Json, Router};
use errors::ApiError;

use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize};
use std::env;
use std::{net::SocketAddr, path::PathBuf};
use tokio::select;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
mod errors;
mod sendfile;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "centarr=debug,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    select! {
        _ = app() => {},
        _ = sendfile::server() => {},
    }
}

async fn app() {
    let app: _ = Router::new()
        .route("/shows", get(get_shows))
        .route("/shows/:showId", get(get_show))
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::debug!("Listening on http://{}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

fn sonarr_url(path: &str) -> String {
    format!("{}{}", env::var("SONARR_URL").unwrap(), path)
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
    size: i64,
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

    #[serde(rename = "watchUrl")]
    watch_url: Option<String>,
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

    Ok(shows.into())
}

async fn get_show(Path(id): Path<i32>, headers: HeaderMap) -> Result<Json<Show>, ApiError> {
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

    let mut episodes = serde_json::from_str::<Vec<Episode>>(&body).unwrap();

    for episode in &mut episodes {
        if episode.episode_file.is_some() {
            let mut file = episode.episode_file.as_mut().unwrap();
            let path = PathBuf::from(file.path.clone());
            file.watch_url = Some(format!(
                "http://{}?file={}",
                headers
                    .get("Host")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace("3000", "3001"),
                urlencoding::encode(path.to_str().unwrap())
            ));
        }
    }

    show.episodes = Some(episodes);

    Ok(show.into())
}

// async fn get_episode(
//     Path(ids): Path<(i32, i32)>,
//     headers: HeaderMap,
// ) -> Result<Json<Episode>, ApiError> {
//     let body = sonarr_client(format!("/episode/{}?seriesId={}", ids.1, ids.0).as_str())
//         .send()
//         .await
//         .map_err(|e| ApiError::empty(500, Some(e.to_string())))?
//         .text()
//         .await
//         .map_err(|e| ApiError::empty(500, Some(e.to_string())))?;

//     let mut episode = serde_json::from_str::<Episode>(&body).unwrap();

//     if let Some(mut file) = episode.episode_file {
//         let mut path = PathBuf::from(file.path.clone());

//         if let Ok(prefix) = env::var("SONARR_DISK_PATH_PREFIX") {
//             path = PathBuf::from(prefix).join(format!(".{}", file.path));
//         }

//         file.watch_url = Some(format!(
//             "http://{}?file={}",
//             headers
//                 .get("Host")
//                 .unwrap()
//                 .to_str()
//                 .unwrap()
//                 .replace("3000", "3001"),
//             urlencoding::encode(path.to_str().unwrap())
//         ));
//         episode.episode_file = Some(file);
//     }

//     Ok(Json(episode))
// }
