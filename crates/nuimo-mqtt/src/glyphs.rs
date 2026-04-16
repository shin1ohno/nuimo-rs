use nuimo::Glyph;

pub fn play() -> Glyph {
    Glyph::from_str(
        "    *    \n\
         **   \n\
         ***  \n\
         **** \n\
         *****\n\
         **** \n\
         ***  \n\
         **   \n\
         *    ",
    )
}

pub fn pause() -> Glyph {
    Glyph::from_str(
        "  **  ** \n\
           **  ** \n\
           **  ** \n\
           **  ** \n\
           **  ** \n\
           **  ** \n\
           **  ** \n\
           **  ** \n\
           **  ** ",
    )
}

pub fn stop() -> Glyph {
    Glyph::from_str(
        " ******* \n\
           ******* \n\
           ******* \n\
           ******* \n\
           ******* \n\
           ******* \n\
           ******* \n\
           ******* \n\
           ******* ",
    )
}

pub fn next() -> Glyph {
    Glyph::from_str(
        "  *   *  \n\
           **  **  \n\
           *** *** \n\
           ********\n\
           ********\n\
           ********\n\
           *** *** \n\
           **  **  \n\
           *   *  ",
    )
}

pub fn previous() -> Glyph {
    Glyph::from_str(
        "  *   *  \n\
           **  **  \n\
          *** *** \n\
         ********\n\
         ********\n\
         ********\n\
          *** *** \n\
           **  **  \n\
           *   *  ",
    )
}

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

/// Volume bar glyph (0-100%).
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
