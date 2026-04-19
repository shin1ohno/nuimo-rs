//! Local glyphs kept inside nuimo-mqtt.
//!
//! Named patterns (play, pause, next, previous, ...) now come from the
//! weave-server glyph registry via `system/glyphs/{name}` retained MQTT
//! messages — see `registry.rs`. Only the glyphs needed before the
//! registry is hydrated (link on connect) or for parametric rendering
//! (volume bar) remain baked in here.

use nuimo::Glyph;

pub fn link() -> Glyph {
    Glyph::from_str(
        "         \n\
          ** **  \n\
         *  * * \n\
         *    * \n\
          *  *  \n\
         *    * \n\
         * *  * \n\
          ** **  \n\
                  ",
    )
}

pub fn empty() -> Glyph {
    Glyph::empty()
}

/// Volume bar glyph (0-100%). Matches the weave `volume_bar` builtin.
pub fn volume(percentage: u8) -> Glyph {
    let bars = ((percentage as f64 / 100.0) * 9.0).round() as usize;
    let mut rows = String::new();
    for row in 0..9 {
        let from_bottom = 8 - row;
        if from_bottom < bars {
            rows.push_str("    *    ");
        } else {
            rows.push_str("         ");
        }
        if row < 8 {
            rows.push('\n');
        }
    }
    Glyph::from_str(&rows)
}
