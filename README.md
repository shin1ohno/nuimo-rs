# nuimo-rs

Rust SDK for the [Senic Nuimo Control](https://www.senic.com/) BLE smart knob, plus an optional MQTT bridge.

## Crates

- `nuimo` — core SDK: BLE discovery, rotation/touch/swipe/button events, LED matrix display. Linux (BlueZ via `bluer`) and macOS (CoreBluetooth via `btleplug`).
- `nuimo-mqtt` — bridge binary that publishes Nuimo events and subscribes to glyph feedback over MQTT. See [SPEC.md](SPEC.md).

## Quick start

```rust
use nuimo::{discover, NuimoDevice, NuimoEvent};

let (mut rx, _handle) = discover().await?;
let found = rx.recv().await.unwrap();
let device = NuimoDevice::new(found.address, &found.adapter);
device.connect().await?;

let mut events = device.events();
while let Ok(event) = events.recv().await {
    if let NuimoEvent::Rotate { delta, .. } = event {
        println!("rotate: {delta}");
    }
}
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
