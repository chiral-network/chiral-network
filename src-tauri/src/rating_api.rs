use axum::{
    extract::{Extension, Path},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::rating_storage::{self, Rating, RatingState};

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmitRatingRequest {
    seeder_wallet: String,
    file_hash: String,
    score: u8,
    comment: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RatingResponse {
    ratings: Vec<Rating>,
    average: f64,
    count: usize,
}

#[derive(Deserialize)]
struct BatchRequest {
    wallets: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchEntry {
    average: f64,
    count: usize,
}

#[derive(Serialize)]
struct BatchResponse {
    ratings: std::collections::HashMap<String, BatchEntry>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the rater wallet address from X-Owner header.
fn get_rater(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-owner")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn compute_average(ratings: &[Rating]) -> f64 {
    if ratings.is_empty() {
        return 0.0;
    }
    let sum: u64 = ratings.iter().map(|r| r.score as u64).sum();
    (sum as f64 / ratings.len() as f64 * 10.0).round() / 10.0
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/ratings — submit a rating
async fn submit_rating(
    Extension(state): Extension<Arc<RatingState>>,
    headers: HeaderMap,
    Json(req): Json<SubmitRatingRequest>,
) -> Response {
    let rater = match get_rater(&headers) {
        Some(r) => r,
        None => return (StatusCode::BAD_REQUEST, "X-Owner header required").into_response(),
    };

    // Validate score
    if req.score < 1 || req.score > 5 {
        return (StatusCode::BAD_REQUEST, "Score must be between 1 and 5").into_response();
    }

    // Validate comment length
    if let Some(ref c) = req.comment {
        if c.len() > 500 {
            return (StatusCode::BAD_REQUEST, "Comment must be 500 characters or less")
                .into_response();
        }
    }

    if req.seeder_wallet.is_empty() {
        return (StatusCode::BAD_REQUEST, "seederWallet is required").into_response();
    }

    if req.file_hash.is_empty() {
        return (StatusCode::BAD_REQUEST, "fileHash is required").into_response();
    }

    // Don't allow rating yourself
    if rater.to_lowercase() == req.seeder_wallet.to_lowercase() {
        return (StatusCode::BAD_REQUEST, "Cannot rate yourself").into_response();
    }

    let mut m = state.manifest.write().await;

    // Check if a rating from this rater for this seeder+file already exists — update if so
    if let Some(existing) = m.ratings.iter_mut().find(|r| {
        r.rater_wallet.to_lowercase() == rater.to_lowercase()
            && r.seeder_wallet.to_lowercase() == req.seeder_wallet.to_lowercase()
            && r.file_hash == req.file_hash
    }) {
        existing.score = req.score;
        existing.comment = req.comment.filter(|c| !c.is_empty());
        existing.created_at = rating_storage::now_secs();
        let updated = existing.clone();
        drop(m);
        state.persist().await;
        println!(
            "[RATING] Updated rating for seeder {} by {} (score: {})",
            req.seeder_wallet, rater, req.score
        );
        return (StatusCode::OK, Json(updated)).into_response();
    }

    // Create new rating
    let rating = Rating {
        id: rating_storage::generate_id(),
        seeder_wallet: req.seeder_wallet.clone(),
        rater_wallet: rater.clone(),
        file_hash: req.file_hash,
        score: req.score,
        comment: req.comment.filter(|c| !c.is_empty()),
        created_at: rating_storage::now_secs(),
    };

    m.ratings.push(rating.clone());
    drop(m);
    state.persist().await;

    println!(
        "[RATING] New rating for seeder {} by {} (score: {})",
        req.seeder_wallet, rater, req.score
    );
    (StatusCode::CREATED, Json(rating)).into_response()
}

/// GET /api/ratings/:wallet — get all ratings for a seeder wallet
async fn get_ratings(
    Extension(state): Extension<Arc<RatingState>>,
    Path(wallet): Path<String>,
) -> Response {
    let m = state.manifest.read().await;
    let ratings: Vec<Rating> = m
        .ratings
        .iter()
        .filter(|r| r.seeder_wallet.to_lowercase() == wallet.to_lowercase())
        .cloned()
        .collect();
    let average = compute_average(&ratings);
    let count = ratings.len();

    Json(RatingResponse {
        ratings,
        average,
        count,
    })
    .into_response()
}

/// POST /api/ratings/batch — batch fetch ratings for multiple wallets
async fn batch_ratings(
    Extension(state): Extension<Arc<RatingState>>,
    Json(req): Json<BatchRequest>,
) -> Response {
    let m = state.manifest.read().await;
    let mut result = std::collections::HashMap::new();

    for wallet in &req.wallets {
        let ratings: Vec<&Rating> = m
            .ratings
            .iter()
            .filter(|r| r.seeder_wallet.to_lowercase() == wallet.to_lowercase())
            .collect();
        let count = ratings.len();
        let average = if count == 0 {
            0.0
        } else {
            let sum: u64 = ratings.iter().map(|r| r.score as u64).sum();
            (sum as f64 / count as f64 * 10.0).round() / 10.0
        };
        result.insert(
            wallet.clone(),
            BatchEntry { average, count },
        );
    }

    Json(BatchResponse { ratings: result }).into_response()
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Create the Rating API router. Uses Extension for state injection.
pub fn rating_routes(state: Arc<RatingState>) -> Router {
    Router::new()
        .route("/api/ratings", post(submit_rating))
        .route("/api/ratings/batch", post(batch_ratings))
        .route("/api/ratings/:wallet", get(get_ratings))
        .layer(Extension(state))
}
