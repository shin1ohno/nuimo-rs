//! Discover Nuimo devices, connect to each, display glyphs on events.
//!
//! Usage:
//!   cargo run -p nuimo --example connect_test

use std::collections::HashSet;
use std::sync::Arc;

use bluer::Address;
use nuimo::{
    discover, DisplayOptions, DisplayTransition, Glyph, NuimoDevice, NuimoEvent, RotationMode,
};
use tokio::sync::Mutex;

fn glyph_play() -> Glyph {
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

fn glyph_pause() -> Glyph {
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

fn glyph_right() -> Glyph {
    Glyph::from_str(
        "  *      \n\
           **     \n\
           ***    \n\
           ****   \n\
           *****  \n\
           ****   \n\
           ***    \n\
           **     \n\
           *      ",
    )
}

fn glyph_left() -> Glyph {
    Glyph::from_str(
        "      *  \n\
              ** \n\
             *** \n\
            **** \n\
           ***** \n\
            **** \n\
             *** \n\
              ** \n\
               *  ",
    )
}

fn glyph_link() -> Glyph {
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

fn glyph_volume(pct: u8) -> Glyph {
    let bars = ((pct as f64 / 100.0) * 9.0).round() as usize;
    let mut s = String::new();
    for row in 0..9 {
        let from_bottom = 8 - row;
        if from_bottom < bars {
            s.push_str("    *    ");
        } else {
            s.push_str("         ");
        }
        if row < 8 { s.push('\n'); }
    }
    Glyph::from_str(&s)
}

async fn show(device: &NuimoDevice, glyph: &Glyph, timeout_ms: u32) {
    let _ = device.display_glyph(glyph, &DisplayOptions {
        brightness: 1.0,
        timeout_ms,
        transition: DisplayTransition::Immediate,
    }).await;
}

async fn handle_device(device: Arc<NuimoDevice>) {
    let id = device.id();

    device.set_rotation_mode(RotationMode::Continuous).await;

    show(&device, &glyph_link(), 3000).await;
    println!("[{}] Displayed link glyph", id);

    if let Some(battery) = device.battery_level().await {
        println!("[{}] Battery: {}%", id, battery);
    }

    println!("[{}] Listening for events...", id);

    let mut events = device.events();
    let mut volume_pct: f64 = 50.0;

    loop {
        match events.recv().await {
            Ok(event) => {
                match &event {
                    NuimoEvent::ButtonDown => {
                        println!("[{}] Button DOWN", id);
                        show(&device, &glyph_play(), 1500).await;
                    }
                    NuimoEvent::ButtonUp => {
                        println!("[{}] Button UP", id);
                        show(&device, &glyph_pause(), 1500).await;
                    }
                    NuimoEvent::Rotate { delta, .. } => {
                        volume_pct = (volume_pct + delta * 100.0).clamp(0.0, 100.0);
                        let pct = volume_pct as u8;
                        println!("[{}] Rotate: delta={:.3} volume={}%", id, delta, pct);
                        show(&device, &glyph_volume(pct), 1000).await;
                    }
                    NuimoEvent::SwipeRight => {
                        println!("[{}] Swipe RIGHT", id);
                        show(&device, &glyph_right(), 1000).await;
                    }
                    NuimoEvent::SwipeLeft => {
                        println!("[{}] Swipe LEFT", id);
                        show(&device, &glyph_left(), 1000).await;
                    }
                    NuimoEvent::SwipeUp => println!("[{}] Swipe UP", id),
                    NuimoEvent::SwipeDown => println!("[{}] Swipe DOWN", id),
                    NuimoEvent::TouchTop => println!("[{}] Touch TOP", id),
                    NuimoEvent::TouchBottom => println!("[{}] Touch BOTTOM", id),
                    NuimoEvent::TouchLeft => println!("[{}] Touch LEFT", id),
                    NuimoEvent::TouchRight => println!("[{}] Touch RIGHT", id),
                    NuimoEvent::LongTouchLeft => println!("[{}] Long Touch LEFT", id),
                    NuimoEvent::LongTouchRight => println!("[{}] Long Touch RIGHT", id),
                    NuimoEvent::LongTouchBottom => println!("[{}] Long Touch BOTTOM", id),
                    NuimoEvent::LongTouchTop => println!("[{}] Long Touch TOP", id),
                    NuimoEvent::Hover { proximity } => {
                        println!("[{}] Hover: proximity={:.2}", id, proximity);
                    }
                    NuimoEvent::FlyLeft => println!("[{}] Fly LEFT", id),
                    NuimoEvent::FlyRight => println!("[{}] Fly RIGHT", id),
                    NuimoEvent::BatteryLevel(level) => println!("[{}] Battery: {}%", id, level),
                    NuimoEvent::Rssi(rssi) => println!("[{}] RSSI: {} dBm", id, rssi),
                    NuimoEvent::Connected => println!("[{}] Connected", id),
                    NuimoEvent::Disconnected => {
                        println!("[{}] Disconnected", id);
                        return;
                    }
                }
            }
            Err(_) => return,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    println!("Scanning for Nuimo devices (Ctrl+C to stop)...");
    let (mut rx, _discovery_handle) = discover().await?;

    // Track which devices have an active connection task
    let active: Arc<Mutex<HashSet<Address>>> = Arc::new(Mutex::new(HashSet::new()));

    while let Some(discovered) = rx.recv().await {
        let addr = discovered.address;

        {
            let guard = active.lock().await;
            if guard.contains(&addr) {
                continue;
            }
        }
        active.lock().await.insert(addr);

        println!("Found: {} ({})", discovered.name, addr);

        let device = Arc::new(NuimoDevice::new(addr, &discovered.adapter));
        let device_clone = device.clone();
        let active_clone = active.clone();

        tokio::spawn(async move {
            println!("[{}] Connecting...", device_clone.id());
            match device_clone.connect().await {
                Ok(()) => {
                    println!("[{}] Connected!", device_clone.id());
                    handle_device(device_clone).await;
                }
                Err(e) => {
                    println!("[{}] Connection failed: {}", device_clone.id(), e);
                }
            }
            // Allow rediscovery after disconnect or connection failure
            active_clone.lock().await.remove(&addr);
            println!("[{}] Ready for rediscovery", addr);
        });
    }

    Ok(())
}
