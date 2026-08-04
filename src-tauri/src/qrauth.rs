// QR login (Opsi-A) — kasir scan QR pakai HP Z One, alur `qrSession` Z One
// yang SUDAH berjalan (api/qr/generate → api/qr/poll). Desktop TIDAK pakai
// QR SSO `device` (api/sso/zpos) karena scanner Z One cuma mengenali token.
//
// Alur:
//   1. start() hit Z One POST /api/qr/generate → { id, token } (token = 60s)
//   2. frontend tampilkan QR berisi https://zone.zomet.my.id/api/qr/authorize?token=<tok>
//      (scanner Z One extract 'token', kirim ke /api/qr/approve → SCANNED)
//   3. kasir tap "Izinkan" di HP → /api/qr/approve action=approve → APPROVED
//   4. poll(id) hit Z One GET /api/qr/poll?id=... → APPROVED → dapat user.email
//   5. desktop POST zpos /api/auth/login-by-email {email} → dapat zpos_token
//      → balik {status:'done', token} → frontend simpan & sync (SyncClient).
//
// Endpoint Z One publik (tanpa cookie): qr/generate & qr/poll.
// login-by-email ZPos butuh email yang SUDAH diverifikasi Z One pasca-approve.

use qrcode::{EcLevel, QrCode, Color};
use serde_json::{json, Value};

// Base Z One (hub akun) — QR session + polling ada di sini, BUKAN di ZPos.
const ZONE_BASE: &str = "https://zone.zomet.my.id";

fn endpoint(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

// Render QR jadi SVG (modul persegi, kontras tinggi utk discan HP).
// Wajib quiet zone >= 4 modul putih di semua sisi (spec QR): tanpa itu
// scanner (jsQR di APK Z One) tak bisa deteksi batas QR -> "tak ada respon".
// qrserver web (QR yang gampang discan) otomatis nambah margin; kita juga.
fn svg_qr(data: &str) -> String {
    let code = QrCode::with_error_correction_level(data.as_bytes(), EcLevel::M).unwrap();
    let w = code.width();
    let pad = 4; // quiet zone 4 modul, spec QR minimum
    let dim = w + 2 * pad;
    let mut svg = String::from(
        "<svg xmlns='http://www.w3.org/2000/svg' width='640' height='640' viewBox='0 0 '",
    );
    svg.push_str(&dim.to_string());
    svg.push(' ');
    svg.push_str(&dim.to_string());
    svg.push_str("' shape-rendering='crispEdges'>");
    svg.push_str("<rect width='100%' height='100%' fill='white'/>"); // bg putih = quiet zone
    for y in 0..w {
        for x in 0..w {
            if code[(x, y)] == Color::Dark {
                // offset ke-dalam sebesar pad -> margin putih 4 modul di tiap sisi
                svg.push_str(&format!(
                    "<rect x='{}' y='{}' width='1' height='1'/>",
                    x + pad,
                    y + pad
                ));
            }
        }
    }
    svg.push_str("</svg>");
    svg
}

// Mulai QR login: minta qrSession ke Z One, render QR (SVG), balik JSON
// {session_id, token, svg, ttl_seconds} utk frontend tampilkan + poll.
// QR konten = URL authorize Z One dgn ?token= supaya scanner Z One (yang
// extract '@token' dari konten QR) bisa cocokkan ke qrSession.
pub fn start(_zpos_base: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(endpoint(ZONE_BASE, "/api/qr/generate"))
        .send()
        .map_err(|e| format!("network: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(format!("HTTP {status}: {body}"));
    }
    let v: Value = resp.json().map_err(|e| format!("json: {e}"))?;
    let id = v["id"].as_str().ok_or("id tidak ada")?.to_string();
    let token = v["token"].as_str().ok_or("token tidak ada")?.to_string();
    let ttl = v["expiresAt"].is_null().then(|| 60u64).unwrap_or(60);

    // QR content: URL authorize di Z One, token sebagai query param.
    // Scanner Z One: extractToken(url) → ?token=<val>. Buka di browser dgn
    // URL ini juga sah (ke halaman authorize), tapi utk scanner cukup token.
    let qr_url = format!("{}/api/qr/authorize?token={}", ZONE_BASE, token);
    let svg = svg_qr(&qr_url);

    serde_json::to_string(&json!({
        "session_id": id,
        "token": token,
        "svg": svg,
        "url": qr_url,
        "ttl_seconds": ttl,
    }))
    .map_err(|e| e.to_string())
}

// Poll status QR login. Balik JSON {status, token?}. status: pending | done | expired.
// Bila APPROVED: ambil user.email → minta zpos_token via ZPos login-by-email.
pub fn poll(session_id: &str, zpos_base: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::new();
    let url = format!(
        "{}?id={}",
        endpoint(ZONE_BASE, "/api/qr/poll"),
        session_id
    );
    let resp = client
        .get(&url)
        .send()
        .map_err(|e| format!("network: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        // 404 = qrSession tidak ada (expired dihapus) → treat sebagai expired.
        if status.as_u16() == 404 {
            return serde_json::to_string(&json!({ "status": "expired" }))
                .map_err(|e| e.to_string());
        }
        return Err(format!("HTTP {status}: {body}"));
    }
    let v: Value = resp.json().map_err(|e| format!("json: {e}"))?;
    let status = v["status"].as_str().unwrap_or("pending").to_string();

    if status != "APPROVED" {
        return serde_json::to_string(&json!({ "status": status.to_lowercase() }))
            .map_err(|e| e.to_string());
    }

    // APPROVED → dapat email user Z One.
    let email = v["user"]["email"].as_str().unwrap_or("").to_string();
    if email.is_empty() {
        return Err("email user tidak ada pada status APPROVED".to_string());
    }

    // Minta zpos_token by email (email sudah diverifikasi Z One pasca-approve).
    let lb = client
        .post(endpoint(zpos_base, "/api/auth/login-by-email"))
        .json(&json!({ "email": email }))
        .send()
        .map_err(|e| format!("network login-by-email: {e}"))?;
    if !lb.status().is_success() {
        let s2 = lb.status();
        let body = lb.text().unwrap_or_default();
        return Err(format!("HTTP {s2}: {body}"));
    }
    let lv: Value = lb.json().map_err(|e| format!("json login-by-email: {e}"))?;
    let token = lv["token"].as_str().ok_or("token tidak ada di login-by-email")?.to_string();

    serde_json::to_string(&json!({ "status": "done", "token": token, "email": email }))
        .map_err(|e| e.to_string())
}
