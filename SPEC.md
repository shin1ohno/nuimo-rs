# nuimo-rs: Nuimo BLE SDK + optional MQTT bridge

## Summary

Senic Nuimo（BLE 物理コントローラー）用の Rust SDK と、その上に載る 2 つの消費側（別リポジトリ）の両方を支えるモジュール群:

- **SDK (`crates/nuimo`)**: BLE で Nuimo に接続し、`NuimoEvent` の stream を購読・`Glyph` で LED を描画する。MQTT 依存なし。いずれの統合経路からも static link で使う
- **MQTT bridge (`crates/nuimo-mqtt`)**: Nuimo の入出力を MQTT に橋渡しする参照実装。N:N クロスホスト運用時の device 側エンドポイント

直結 edge-agent 経路（推奨）では、`nuimo` SDK を [`edge-agent`](https://github.com/shin1ohno/edge-agent) が静的リンクして直接 BLE を扱う。MQTT 経路を使う場合のみ `nuimo-mqtt` を動かす。

## Requirements

### Must

1. **BLE 接続**: D-Bus 経由で bluez にアクセスし、Nuimo を discover + connect
2. **全イベント提供**: Button / Rotate / Swipe / Touch / LongTouch / Fly / Hover / Battery / RSSI / Connected|Disconnected を `broadcast::Receiver<NuimoEvent>` で配信
3. **9x9 Glyph 描画**: `Glyph::from_str("...9 行の ASCII...")` + `DisplayOptions { brightness, timeout_ms, transition }` で LED 表示
4. **Rotation mode 切替**: `Continuous` / `Clamped { min, max }`
5. **再接続安全**: D-Bus property 監視で切断検知、再接続で characteristic 再サブスクライブ

### Should

6. **複数 Nuimo 並存**: BLE アドレスで一意識別、1 プロセスから複数台を扱える
7. **サンプル**: `crates/nuimo/examples/connect_test.rs` で最小接続 + イベント dump

### Could

8. **Fly と Swipe の区別**: 現在は edge-agent / nuimo-mqtt 双方で `FlyLeft/Right` を `SwipeLeft/Right` に統合している。用途分離したくなったら個別に

## Non-requirements

- ルーティング / マッピング（`edge-agent` または `weave-engine` が担当）
- サービス接続（Roon / Hue 等は別 adapter）
- MQTT 以外のプロトコル bridge（必要なら別 crate）

## Technical Approach

### 経路 A: 直結 edge-agent 経由（推奨）

```
[edge-agent host]
  edge-agent binary
   ├ nuimo (path dep or git dep)   ← この crate
   │   └ bluez / D-Bus
   ├ routing engine
   ├ adapter-roon                  → Roon Core (MOO RPC)
   └ ws client                     → weave-server (/ws/edge)
```

edge-agent が `nuimo::discover() → NuimoDevice::connect() → device.events()` をそのまま使い、ユーザー入力を `InputPrimitive` に変換して routing engine に投げる。フィードバック glyph は weave から受信した registry から名前参照で描画。

### 経路 B: MQTT bridge

```
[nuimo-mqtt host]                            [weave (-engine)]           [roon-hub host]
  nuimo-mqtt binary                           mosquitto
   ├ nuimo                                     ↑
   └ MQTT client   ──►  device/nuimo/{id}/input/{primitive}
                                               │
                   ◄──  device/nuimo/{id}/feedback/{type}
```

- 入力 primitive → `device/nuimo/{id}/input/{primitive}` に publish（QoS 0）
- フィードバック: `device/nuimo/{id}/feedback/{type}` を subscribe、ペイロードから glyph を決定して LED 描画
- `device/nuimo/{id}/state/{connected|battery|rssi}` を retained publish

どちらを選ぶかの判断軸は `weave/SPEC.md` 参照。

### イベント → InputPrimitive 対応（両経路で共通）

| `NuimoEvent` | `InputPrimitive` / MQTT primitive |
|---|---|
| `ButtonDown` | `Press` / `press` |
| `ButtonUp` | `Release` / `release` |
| `Rotate { delta }` | `Rotate { delta }` / `rotate` (payload: `{"delta": f64}`) |
| `SwipeUp`/`Down`/`Left`/`Right` | `Swipe { direction }` / `swipe_{up,down,left,right}` |
| `TouchTop` 等 | `Touch { area }` / `touch_{top,bottom,left,right}` |
| `LongTouch*` | `LongTouch { area }` / `long_touch_{...}` |
| `FlyLeft`/`Right` | `Swipe { Left/Right }`（現状は吸収） |
| `Hover { proximity }` | `Hover { proximity }` / `hover` |
| `BatteryLevel(u8)` | state のみ |
| `Rssi(i16)` | state のみ |

### Crate 構成

```
nuimo-rs/
├ crates/
│   ├ nuimo/               ← SDK。BLE + Glyph + Event。MQTT 非依存
│   └ nuimo-mqtt/          ← 経路 B の device-side binary
└ SPEC.md (本ドキュメント)
```

edge-agent 側は `nuimo` のみに依存する（`nuimo-mqtt` は使わない）。

## Edge Cases

| ケース | 動作 |
|---|---|
| Nuimo 範囲外 / 電源 OFF | `Disconnected` イベント、以降の `display_glyph` はエラー |
| 再ペアリング | D-Bus adapter 経由で自動、ユーザー操作不要 |
| 複数 Nuimo | 別 `NuimoDevice::new(addr)` で独立購読 |
| MQTT broker 不在（経路 B） | nuimo-mqtt は接続リトライ、BLE イベントはロスト |
| glyph 命名衝突 | weave の glyph registry は name が PK。衝突は上書き |

## Acceptance Criteria

1. Nuimo を 1 台接続して全イベント種別が emit される（examples/connect_test.rs で確認可能）
2. 9x9 ASCII から LED 描画が正しく出る
3. 切断 → 再接続で notification が自動再購読される
4. edge-agent 経由で Nuimo rotate が Roon Living zone の volume を変える（<100ms）
5. nuimo-mqtt 経由で `device/nuimo/{id}/input/rotate` に publish が出る
