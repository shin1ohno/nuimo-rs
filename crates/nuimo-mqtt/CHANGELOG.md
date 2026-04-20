# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/shin1ohno/nuimo-rs/releases/tag/nuimo-mqtt-v0.1.0) - 2026-04-20

### Other

- pin nuimo path dep with crates.io version fallback
- depend on weave-contracts from crates.io
- rustfmt pass + allow FromStr lint on Glyph
- consume weave's system/glyphs/+ for feedback patterns
- align MQTT topics with weave SPEC
- implement MQTT bridge for Nuimo BLE controller
- Initial workspace: nuimo crate with GATT definitions, glyph encoding, event types
