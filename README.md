# nuimo-rs

Rust SDK for the [Senic Nuimo Control](https://www.senic.com/) BLE smart knob, plus an optional MQTT bridge.

## Crates

- `nuimo` — core SDK: BLE discovery, the full upstream gesture vocabulary (rotation, button, swipe ×4, touch ×4, long-touch ×4, fly ×2, hover proximity, battery), and 9×9 LED matrix display. Linux backend via `bluer`/BlueZ, macOS via `btleplug`/CoreBluetooth.
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
    match event {
        NuimoEvent::Rotate { delta, .. } => println!("rotate: {delta}"),
        NuimoEvent::ButtonDown => println!("press"),
        NuimoEvent::SwipeLeft => println!("swipe left"),  // physical surface
        NuimoEvent::FlyLeft => println!("fly left"),       // in-air wave
        NuimoEvent::Hover { proximity } => println!("hover: {proximity:.2}"),
        _ => {}
    }
}
```

`SwipeLeft`/`SwipeRight` (finger touching the surface) and `FlyLeft`/`FlyRight` (hand waved above the device) are reported as separate events to match the upstream
[`getsenic/nuimo-linux-python`](https://github.com/getsenic/nuimo-linux-python) vocabulary — consumers route them independently.

### Multiple devices on one host

`discover()` reports every Nuimo it sees on a `tokio::sync::mpsc::Receiver`; consumers that want multiple devices on the same host (e.g. one knob per room) keep draining the channel and instantiate one `NuimoDevice` per discovered address. Each `NuimoDevice` owns its own BLE session; events, LED writes, and reconnects are independent. The Linux bluer adapter is shared but multi-peripheral safe.

## Operational Notes

### Latency floor

- BLE connection interval on Nuimo peripherals is typically 7.5–30 ms. That's the hard lower bound on how often `NuimoEvent::Rotate { delta }` can physically arrive — the SDK can't deliver ticks faster than the radio schedules them.
- LED matrix writes are one GATT write per frame. Back-to-back writes are rate-limited by the same connection interval; callers that stream high-frequency updates (e.g. volume-bar animation tied to rotate deltas) should coalesce on the consumer side.
- Disconnects are frequent and unannounced (low battery, out-of-range, host BLE stack hiccup). `NuimoDevice` does not retry automatically — the consumer owns the reconnect strategy. Watch for `NuimoEvent::Disconnected` on the events channel and call `device.connect()` again with backoff.

### Compatibility policy

- `nuimo` is the public Rust SDK. Treat `NuimoEvent`, `NuimoDevice`, and the `discover()` channel signature as the API surface.
- Today's semver rules:
  - **MINOR** — struct field addition on event payloads (downstream match arms keep compiling if they use `..`).
  - **MAJOR** — `NuimoEvent` enum variant addition (pattern matches without a `_ =>` arm break at compile time). Consumers that want forward-compat can add `_ => {}` today and we can relax this later with `#[non_exhaustive]`.
  - **MAJOR** — method rename, trait bound change, or backend swap (bluer ↔ btleplug selection, OS gating).
- `nuimo-mqtt` is a consumer of `nuimo`, not part of the SDK. It ships on a separate version line and its own CHANGELOG.

## Downstream

- [`shin1ohno/edge-agent`](https://github.com/shin1ohno/edge-agent) — primary consumer. Per-host Rust binary that uses `nuimo` for BLE input and routes events through a configurable engine to Roon / Philips Hue / macOS audio / iPad media services.
- The `nuimo-protocol` crate inside edge-agent's workspace shares the wire-format parsers with the iOS port; this crate (`nuimo`) wraps them for a desktop BLE stack.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
