// QR login (device pairing via Z One SSO) — endpoint auth ZPos, tanpa cookie.
// Alur:
//   1. start() POST /api/auth/qr-request → device_code + url (ABSOLUT ke Z One)
//   2. frontend tampilkan QR berisi url tsb (kasir scan pakai HP)
//   3. kasir login Z One di HP, Z One redirect ke /sso?token=...&device=... →
//      server sangkut zpos_token ke baris device_login
//   4. poll() GET /api/auth/qr-poll?code=... → {status:'done', token} → desktop
//      simpan token & lanjut sync (pakai SyncClient yang sudah ada).
//
// Endpoint ini TANPA cookie (qr-request/qr-poll publik). Dipakai reqwest
// blocking (worker thread Tauri) — konsisten dgn sync.rs.

use qrcode::{EcLevel, QrCode};
use serde_json::Value;

fn endpoint(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

// Render QR jadi SVG (modul persegi, kontras tinggi utk discan HP).
// qrcode crate kasih grid bool; dipakai SVG ringan & krisp, tanpa canvas.
fn svg_qr(data: &str) -> String {
    let code = QrCode::with_error_correction_level(data.as_bytes(), EcLevel::M).unwrap();
    let w = code.width();
    let mut svg = String::from(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 ",
    );
    svg.push_str(&w.to_string());
    svg.push(' ');
    svg.push_str(&w.to_string());
    svg.push_str("' shape-rendering='crispEdges'>");
    for y in 0..w {
        for x in 0..w {
            if code[(x, y)] {
                svg.push_str(&format!(
                    "<rect x='{}' y='{}' width='1' height='1'/>",
                    x, y
                ));
            }
        }
    }
    svg.push_str("</svg>");
    svg
}

// Mulai QR login: minta device_code + url ke server, balik JSON utk frontend.
// base_url = base server ZPos (ex https://zpos.zomet.my.id), dipakai utk hit
// endpoint. Server balik field "url" yang SUDAH ABSOLUT (ke Z One), jangan
// digabung ulang dgn base_url (bug ganda-domain).
pub fn start(base_url: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(endpoint(base_url, "/api/auth/qr-request"))
        .send()
        .map_err(|e| format!("network: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }
    let v: Value = resp.json().map_err(|e| format!("json: {e}"))?;
    let device_code = v["device_code"].as_str().ok_or("device_code tidak ada")?.to_string();
    let url = v["url"].as_str().ok_or("url tidak ada")?.to_string();
    let svg = svg_qr(&url);
    let ttl = v["ttl_seconds"].as_u64().unwrap_or(120);

    serde_json::to_string(&serde_json::json!({
        "device_code": device_code,
        "svg": svg,
        "url": url,
        "ttl_seconds": ttl,
    }))
    .map_err(|e| e.to_string())
}

// Poll status QR login. Balik JSON {status, token?}. status: pending | done | expired.
pub fn poll(base_url: &str, device_code: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::new();
    let url = format!("{}?code={}", endpoint(base_url, "/api/auth/qr-poll"), device_code);
    let resp = client
        .get(url)
        .send()
        .map_err(|e| format!("network: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }
    resp.text().map_err(|e| format!("body: {e}"))
}
