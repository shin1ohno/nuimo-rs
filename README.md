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

## Operational Notes

### Latency floor

- BLE connection interval on Nuimo peripherals is typically 7.5–30 ms. That's the hard lower bound on how often `NuimoEvent::Rotate { delta }` can physically arrive — the SDK can't deliver ticks faster than the radio schedules them.
- LED matrix writes are one GATT write per frame. Back-to-back writes are rate-limited by the same connection interval; callers that stream high-frequency updates (e.g. volume-bar animation tied to rotate deltas) should coalesce on the consumer side.
- Disconnects are frequent and unannounced (low battery, out-of-range, host BLE stack hiccup). `NuimoDevice` does not retry automatically today — the consumer owns the reconnect strategy. `@see` the Events channel draining to detect the end of a session.

### Compatibility policy

- `nuimo` is the public Rust SDK. Treat `NuimoEvent`, `NuimoDevice`, and the `discover()` channel signature as the API surface.
- Today's semver rules:
  - **MINOR** — struct field addition on event payloads (downstream match arms keep compiling if they use `..`).
  - **MAJOR** — `NuimoEvent` enum variant addition (pattern matches without a `_ =>` arm break at compile time). Consumers that want forward-compat can add `_ => {}` today and we can relax this later with `#[non_exhaustive]`.
  - **MAJOR** — method rename, trait bound change, or backend swap (bluer ↔ btleplug selection, OS gating).
- `nuimo-mqtt` is a consumer of `nuimo`, not part of the SDK. It ships on a separate version line and its own CHANGELOG.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
