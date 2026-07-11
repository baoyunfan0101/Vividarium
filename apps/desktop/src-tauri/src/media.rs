use std::fs;

use tauri::http::{Request, Response, StatusCode};

use crate::state;

pub fn handle(request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    match response(request.uri().path()) {
        Ok(response) => response,
        Err((status, message)) => Response::builder()
            .status(status)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(message.into_bytes())
            .expect("valid media error response"),
    }
}

fn response(path: &str) -> Result<Response<Vec<u8>>, (StatusCode, String)> {
    let state = state::global().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "application state is not ready".into(),
    ))?;
    let decoded_path = percent_encoding::percent_decode_str(path).decode_utf8_lossy();
    let parts = decoded_path
        .trim_matches('/')
        .split('/')
        .collect::<Vec<_>>();
    if parts.len() != 2 {
        return Err((StatusCode::BAD_REQUEST, "invalid media URL".into()));
    }
    let photo_id = parts[1]
        .parse::<i64>()
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid photo id".into()))?;
    let file = match parts[0] {
        "photo" => phytoindex_core::photos::photo_file_path(&state.database, photo_id),
        "thumbnail" => phytoindex_core::photos::get_or_create_thumbnail(
            &state.database,
            photo_id,
            &state.thumbnail_dir,
        ),
        _ => return Err((StatusCode::NOT_FOUND, "unknown media resource".into())),
    }
    .map_err(|error| (StatusCode::NOT_FOUND, error.to_string()))?;
    let content_type = mime_guess::from_path(&file)
        .first_or_octet_stream()
        .essence_str()
        .to_string();
    let body = fs::read(file).map_err(|error| (StatusCode::NOT_FOUND, error.to_string()))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", content_type)
        .header("Cache-Control", "public, max-age=31536000, immutable")
        .body(body)
        .expect("valid media response"))
}
