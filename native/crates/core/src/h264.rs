//! Hardware H.264 live view: the capture source produces Annex B access units next to the JPEG
//! frames; viewers get them over the /video WebSocket and decode in hardware (WebCodecs).
//!   macOS          VideoToolbox, inside the capture helper (stream2)
//!   Linux/Windows  a second ffmpeg output ("video.h264_encoder": libx264 | h264_vaapi |
//!                  h264_nvenc | h264_qsv | h264_mf ...), access units delimited by AUD NALs
//! A viewer starts at the next keyframe (at most ~1 s), so it never decodes from a gap.

use crate::server::S;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use serde_json::json;

/// NAL unit types present in an Annex B access unit.
pub fn nal_types(au: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    let mut i = 0;
    while i + 3 < au.len() {
        if au[i] == 0 && au[i + 1] == 0 && (au[i + 2] == 1 || (au[i + 2] == 0 && au[i + 3] == 1)) {
            let hdr = if au[i + 2] == 1 { i + 3 } else { i + 4 };
            if hdr < au.len() {
                out.push(au[hdr] & 0x1F);
            }
            i = hdr;
        } else {
            i += 1;
        }
    }
    out
}

pub fn is_keyframe(au: &[u8]) -> bool {
    nal_types(au).iter().any(|t| *t == 5 || *t == 7)
}

/// WebCodecs codec string ("avc1.PPCCLL") from the SPS in a keyframe access unit.
pub fn codec_string(au: &[u8]) -> Option<String> {
    let mut i = 0;
    while i + 4 < au.len() {
        if au[i] == 0 && au[i + 1] == 0 && au[i + 2] == 1 && au[i + 3] & 0x1F == 7 && i + 6 < au.len() {
            return Some(format!("avc1.{:02x}{:02x}{:02x}", au[i + 4], au[i + 5], au[i + 6]));
        }
        i += 1;
    }
    None
}

/// Called by the capture source for every H.264 access unit.
pub fn push(hub: &crate::video::FrameHub, au: Bytes) {
    if is_keyframe(&au) {
        *hub.h264_config.lock().unwrap() = Some(au.clone());
    }
    let _ = hub.h264.send(au);
}

/// Non-macOS sources start their own H.264 output here (see video.rs); nothing to do on macOS.
pub async fn ws_video(State(b): State<S>, ws: WebSocketUpgrade) -> Response {
    // which console: decided now, while the request still names it
    match b.target() {
        Ok(c) => ws.on_upgrade(move |sock| video_socket(c, sock)),
        Err(e) => axum::response::IntoResponse::into_response((axum::http::StatusCode::NOT_FOUND, e.to_string())),
    }
}

async fn video_socket(c: std::sync::Arc<crate::console::Console>, sock: WebSocket) {
    let (mut tx, mut rx) = sock.split();
    let mut aus = c.frames.h264.subscribe();
    // drain client messages (close/pings)
    let reader = tokio::spawn(async move { while let Some(Ok(_)) = rx.next().await {} });
    let mut started = false;
    loop {
        let au = match aus.recv().await {
            Ok(a) => a,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                started = false; // fell behind: resync at the next keyframe
                continue;
            }
            Err(_) => break,
        };
        if !started {
            if !is_keyframe(&au) {
                continue;
            }
            let codec = codec_string(&au).unwrap_or_else(|| "avc1.640028".into());
            let c = c.video_cfg.lock().unwrap().clone();
            let init = json!({"t": "init", "codec": codec, "width": c["width"], "height": c["height"], "format": "annexb"});
            if tx.send(Message::Text(init.to_string().into())).await.is_err() {
                break;
            }
            started = true;
        }
        if tx.send(Message::Binary(au)).await.is_err() {
            break;
        }
    }
    reader.abort();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nal_parsing() {
        let au = [0, 0, 0, 1, 0x27, 0x64, 0x00, 0x28, 0xAA, 0, 0, 0, 1, 0x28, 0xBB, 0, 0, 1, 0x25, 0xCC];
        assert_eq!(nal_types(&au), vec![7, 8, 5]);
        assert!(is_keyframe(&au));
        assert_eq!(codec_string(&au).unwrap(), "avc1.640028");
        assert!(!is_keyframe(&[0, 0, 0, 1, 0x21, 0x99]));
    }
}
