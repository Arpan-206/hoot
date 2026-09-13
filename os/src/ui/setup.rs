//! The Wi-Fi setup screen: instructions and a QR code that joins the hotspot.

use sprig_gfx::{CELL_HEIGHT, Framebuffer};

use crate::net::PORTAL_SSID;
use crate::ui::{qr, theme};

/// Scanning this joins the open hotspot on iOS and Android.
const WIFI_QR: &str = "WIFI:T:nopass;S:Sprig-Setup;;";

pub fn draw(fb: &mut Framebuffer) {
    theme::screen(fb, "Wi-Fi setup", "");
    let lines: [(&str, bool); 11] = [
        ("Setup mode", true),
        ("", false),
        ("1 Join wifi:", true),
        (PORTAL_SSID, false),
        ("", false),
        ("2 Page opens", true),
        ("or open", false),
        ("192.168.4.1", false),
        ("", false),
        ("3 Pick wifi", true),
        ("and Save", false),
    ];
    let mut y = theme::CONTENT_Y - 2;
    for (text, strong) in lines {
        let color = if strong { theme::TEXT } else { theme::ACCENT };
        fb.draw_text(2, y, text, color, None);
        y += CELL_HEIGHT;
    }
    // Version 2 code at 3 px per module with a 1-module border: 81 px.
    qr::draw(fb, 77, theme::CONTENT_Y - 2, 3, 1, WIFI_QR);
}
