//! Measure input with the bench's own eyes.
//! probe(): press buttons and watch the live video until the picture changes (latency, and
//! whether a press of that length registered). calibrate(): sweep hold times with a
//! forward/back pair that visibly moves something and save a safe minimum press.

use crate::bench::Bench;
use crate::config::{state_dir, write_json};
use crate::video::{Frame, FrameHub};
use anyhow::{anyhow, bail, Result};
use image::GrayImage;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// 1.5x the shortest press the Switch 2 registers reliably (50 ms, measured).
pub const DEFAULT_MIN_PRESS_MS: u64 = 75;

/// The next frame newer than `after` (waits up to `timeout`).
pub async fn next_frame(hub: &FrameHub, after: u64, timeout: Duration) -> Result<Frame> {
    let mut rx = hub.latest.subscribe();
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(f) = rx.borrow_and_update().clone() {
            if f.seq > after {
                return Ok(f);
            }
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || tokio::time::timeout(left, rx.changed()).await.is_err() {
            return Err(anyhow!("no live video (no new frame)"));
        }
    }
}

pub async fn small(f: &Frame) -> Result<GrayImage> {
    let j = f.jpeg.clone();
    tokio::task::spawn_blocking(move || -> Result<GrayImage> { Ok(shrink(&luma(&j)?, 160, 90)) }).await?
}

/// Only the brightness of a JPEG: no colour conversion or chroma upsampling, which is most of a
/// full decode. Comparing frames needs nothing more.
pub fn luma(jpeg: &[u8]) -> Result<GrayImage> {
    use zune_jpeg::zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
    let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::Luma)
        .set_max_width(16384).set_max_height(16384);
    let mut d = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(jpeg), opts);
    match d.decode() {
        Ok(px) => {
            let (w, h) = d.dimensions().ok_or_else(|| anyhow!("JPEG without a size"))?;
            GrayImage::from_raw(w as u32, h as u32, px).ok_or_else(|| anyhow!("JPEG size and data disagree"))
        }
        // anything zune can't take (rare encoders): the general decoder
        Err(_) => Ok(image::load_from_memory(jpeg)?.to_luma8()),
    }
}

/// Area-average downscale (exact box filter for 1080p/1440p/4K -> 160x90). Written out here because
/// generic image resizing is compiled into this crate and is very slow in debug builds.
pub fn shrink(im: &GrayImage, w: u32, h: u32) -> GrayImage {
    let (sw, sh) = (im.width() as usize, im.height() as usize);
    let src = im.as_raw();
    let mut sums = vec![0u32; (w * h) as usize];
    let mut counts = vec![0u32; (w * h) as usize];
    let xmap: Vec<usize> = (0..sw).map(|x| x * w as usize / sw).collect();
    for (y, row) in src.chunks_exact(sw).enumerate() {
        let base = (y * h as usize / sh) * w as usize;
        for (x, &p) in row.iter().enumerate() {
            let i = base + xmap[x];
            sums[i] += p as u32;
            counts[i] += 1;
        }
    }
    GrayImage::from_fn(w, h, |x, y| {
        let i = (y * w + x) as usize;
        image::Luma([(sums[i] / counts[i].max(1)) as u8])
    })
}

/// Mean difference of the most-changed cell in an 8x6 grid, so a small local change (one icon
/// highlighted) is not averaged away.
pub fn diff(a: &GrayImage, b: &GrayImage) -> f64 {
    let (w, h) = (a.width(), a.height());
    let (cw, ch) = (w / 8, h / 6);
    let mut best = 0.0f64;
    for cx in 0..8 {
        for cy in 0..6 {
            let mut s = 0u64;
            for y in cy * ch..(cy + 1) * ch {
                for x in cx * cw..(cx + 1) * cw {
                    s += (a.get_pixel(x, y)[0] as i32 - b.get_pixel(x, y)[0] as i32).unsigned_abs() as u64;
                }
            }
            best = best.max(s as f64 / (cw * ch) as f64);
        }
    }
    best
}

fn region_of(img: &GrayImage, r: Option<[f64; 4]>) -> GrayImage {
    let Some(r) = r else { return img.clone() };
    let (w, h) = (img.width() as f64, img.height() as f64);
    let x = (r[0] * w) as u32;
    let y = (r[1] * h) as u32;
    let cw = ((r[2] * w) as u32).max(8).min(img.width() - x.min(img.width() - 8));
    let ch = ((r[3] * h) as u32).max(6).min(img.height() - y.min(img.height() - 6));
    image::imageops::crop_imm(img, x.min(img.width() - cw), y.min(img.height() - ch), cw, ch).to_image()
}

/// Wait until the picture (or part of it) changes from how it looks now ("change"), or
/// until it stops moving for `stable_ms` ("stable"). The basis of "press, then wait for
/// the screen to react" without guessing a sleep.
pub async fn wait_screen(hub: &FrameHub, until: &str, timeout: Duration, stable_ms: u64, region: Option<[f64; 4]>) -> Result<Value> {
    let t0 = Instant::now();
    let mut f = hub.latest().filter(|f| f.at.elapsed() < Duration::from_secs(5)).ok_or_else(|| anyhow!("no live video to watch"))?;
    let shrink_r = |g: GrayImage| region_of(&g, region);
    let ms = |t: Instant| t.elapsed().as_millis() as u64;
    match until {
        "change" => {
            let base = shrink_r(small(&f).await?);
            // noise floor: how much the picture moves by itself (video noise, animations)
            let mut floor = 0.0f64;
            for _ in 0..2 {
                if let Ok(nf) = next_frame(hub, f.seq, Duration::from_secs(1)).await {
                    f = nf;
                    floor = floor.max(diff(&shrink_r(small(&f).await?), &base));
                }
            }
            let threshold = (floor * 3.0 + 2.0).max(4.0);
            let mut peak = 0.0f64;
            while t0.elapsed() < timeout {
                let Ok(nf) = next_frame(hub, f.seq, Duration::from_millis(500)).await else { continue };
                f = nf;
                let d = diff(&shrink_r(small(&f).await?), &base);
                peak = peak.max(d);
                if d > threshold {
                    return Ok(json!({"until": "change", "ok": true, "waited_ms": ms(t0), "diff": (d * 100.0).round() / 100.0, "frame": f.seq}));
                }
            }
            Ok(json!({"until": "change", "ok": false, "waited_ms": ms(t0), "diff": (peak * 100.0).round() / 100.0,
                      "message": "the screen did not change"}))
        }
        "stable" => {
            let mut prev = shrink_r(small(&f).await?);
            let mut calm_since = Instant::now();
            while t0.elapsed() < timeout {
                let Ok(nf) = next_frame(hub, f.seq, Duration::from_millis(500)).await else { continue };
                f = nf;
                let cur = shrink_r(small(&f).await?);
                if diff(&cur, &prev) >= 1.2 { calm_since = Instant::now(); }
                prev = cur;
                if calm_since.elapsed() >= Duration::from_millis(stable_ms) {
                    return Ok(json!({"until": "stable", "ok": true, "waited_ms": ms(t0), "frame": f.seq}));
                }
            }
            Ok(json!({"until": "stable", "ok": false, "waited_ms": ms(t0), "message": "the screen kept moving"}))
        }
        _ => bail!("until must be change or stable"),
    }
}

/// Wait until the picture stops moving (menus animate after each press).
pub async fn settle(hub: &FrameHub) -> Result<Frame> {
    let mut f = hub.latest().ok_or_else(|| anyhow!("no live video to watch"))?;
    let mut prev = small(&f).await?;
    let (mut calm, t0) = (0, Instant::now());
    while calm < 4 && t0.elapsed() < Duration::from_secs(2) {
        f = next_frame(hub, f.seq, Duration::from_secs(2)).await?;
        let cur = small(&f).await?;
        calm = if diff(&cur, &prev) < 1.2 { calm + 1 } else { 0 };
        prev = cur;
    }
    Ok(f)
}

pub async fn probe(b: &Bench, buttons: &[String], hold_ms: u64, timeout_ms: u64, source: &str) -> Result<Value> {
    let hub = &b.frames;
    let mut f = settle(hub).await?;
    let base = small(&f).await?;
    let mut floor = 0.0f64;
    for _ in 0..3 {
        f = next_frame(hub, f.seq, Duration::from_secs(2)).await?;
        floor = floor.max(diff(&small(&f).await?, &base));
    }
    let threshold = (floor * 3.0 + 2.0).max(4.0); // per cell: a moved highlight is ~10-40
    let t0 = Instant::now();
    let press = b.hub.press(buttons, hold_ms, source, true);
    let watch = async {
        let (mut peak, mut latency) = (0.0f64, None);
        while t0.elapsed() < Duration::from_millis(timeout_ms) {
            let Ok(nf) = next_frame(hub, f.seq, Duration::from_secs(1)).await else { continue };
            f = nf;
            let d = diff(&small(&f).await?, &base);
            peak = peak.max(d);
            if d > threshold {
                latency = Some((f.at.duration_since(t0).as_secs_f64() * 10000.0).round() / 10.0);
                break;
            }
        }
        anyhow::Ok((peak, latency))
    };
    let (pr, wr) = tokio::join!(press, watch);
    pr?;
    let (peak, latency) = wr?;
    Ok(json!({"buttons": buttons, "hold_ms": hold_ms, "changed": latency.is_some(), "latency_ms": latency,
              "diff": (peak * 100.0).round() / 100.0, "noise": (floor * 100.0).round() / 100.0}))
}

fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    Some(((if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }) * 10.0).round() / 10.0)
}

pub async fn calibrate(b: &Bench, forward: &[String], back: &[String], holds: &[u64], repeats: u32) -> Result<Value> {
    let mut sweep = vec![];
    let mut all = vec![];
    let mut reliable = vec![];
    for &hold in holds {
        let (mut ok, mut lat) = (0, vec![]);
        for _ in 0..repeats {
            let r = probe(b, forward, hold, 1500, "calibrate").await?;
            if r["changed"].as_bool() == Some(true) {
                ok += 1;
                lat.push(r["latency_ms"].as_f64().unwrap_or(0.0));
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
            probe(b, back, 120, 1500, "calibrate").await?; // always restore with a sure press
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        b.logs.add("bench", format!("calibration {{hold_ms: {hold}, registered: {ok}/{repeats}, latency_ms: {:?}}}", median(&mut lat.clone())));
        if ok == repeats {
            reliable.push(hold);
        }
        all.extend(lat);
        sweep.push(json!({"hold_ms": hold, "ok": ok, "of": repeats}));
    }
    let shortest = reliable.iter().min().copied();
    let p90 = if all.len() >= 5 {
        let mut s = all.clone();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Some((s[(s.len() as f64 * 0.9) as usize - 1] * 10.0).round() / 10.0)
    } else {
        None
    };
    let out = json!({
        "measured_at": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        "shortest_reliable_press_ms": shortest,
        "min_press_ms": shortest.map_or(DEFAULT_MIN_PRESS_MS, |s| clamp_min_press((s as f64 * 1.5).round() as u64)),
        "latency_ms_median": median(&mut all),
        "latency_ms_p90": p90,
        "sweep": sweep,
    });
    write_json(&state_dir().join("calibration.json"), &out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_diff_sees_local_change() {
        let a = GrayImage::from_pixel(160, 90, image::Luma([100]));
        let mut b = a.clone();
        for y in 0..15 {
            for x in 0..20 {
                b.put_pixel(x, y, image::Luma([140]));
            }
        }
        assert!((diff(&a, &b) - 40.0).abs() < 0.01);
        assert_eq!(diff(&a, &a), 0.0);
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
    }
}

#[cfg(test)]
mod frame_tests {
    /// NS2_F0=a.jpg NS2_F1=b.jpg cargo test -- --ignored diff_files
    #[test]
    #[ignore]
    fn diff_files() {
        let load = |k: &str| {
            let j = std::fs::read(std::env::var(k).unwrap()).unwrap();
            let t = std::time::Instant::now();
            let d = image::load_from_memory(&j).unwrap();
            let t1 = t.elapsed();
            let im = d.to_luma8();
            let t2 = t.elapsed();
            let r = super::shrink(&im, 160, 90);
            println!("decode {t1:?} luma {t2:?} resize {:?}", t.elapsed());
            r
        };
        let (a, b) = (load("NS2_F0"), load("NS2_F1"));
        println!("rust diff {:.3}", super::diff(&a, &b));
    }
}

/// The minimum press every tap is stretched to: never so short the console misses it, never so
/// long that a bad calibration run (or an edited file) makes every button feel stuck.
pub fn clamp_min_press(ms: u64) -> u64 {
    ms.clamp(40, 300)
}

#[cfg(test)]
mod luma_tests {
    use image::{codecs::jpeg::JpegEncoder, RgbImage};

    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let im = RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 7 % 256) as u8, (y * 3 % 256) as u8, ((x ^ y) % 256) as u8]));
        let mut out = vec![];
        JpegEncoder::new_with_quality(&mut out, 85).encode_image(&im).unwrap();
        out
    }

    #[test]
    fn same_picture_as_the_full_decode() {
        // grey picture: both decoders weigh colours the same then (JPEG brightness uses BT.601,
        // `image` Rec.709; frames are only ever compared with frames from the same decoder)
        let im = RgbImage::from_fn(320, 180, |x, y| { let v = ((x * 5 + y * 3) % 256) as u8; image::Rgb([v, v, v]) });
        let mut j = vec![];
        JpegEncoder::new_with_quality(&mut j, 85).encode_image(&im).unwrap();
        let a = super::luma(&j).unwrap();
        let b = image::load_from_memory(&j).unwrap().to_luma8();
        assert_eq!(a.dimensions(), b.dimensions());
        // the frame diff between the two decoders is noise, far under any change threshold
        let d = super::diff(&super::shrink(&a, 160, 90), &super::shrink(&b, 160, 90));
        assert!(d < 2.0, "diff {d}");
    }

    /// cargo test --release -p ns2bench-core luma_speed -- --ignored --nocapture
    #[test]
    #[ignore]
    fn luma_speed() {
        let j = jpeg(3840, 2160);
        let t = std::time::Instant::now();
        for _ in 0..10 { let _ = image::load_from_memory(&j).unwrap().to_luma8(); }
        let full = t.elapsed() / 10;
        let t = std::time::Instant::now();
        for _ in 0..10 { let _ = super::luma(&j).unwrap(); }
        let fast = t.elapsed() / 10;
        println!("4K frame: full decode {full:?}, brightness only {fast:?}");
    }
}
