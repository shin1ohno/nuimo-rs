# nu-router: IoT Device ↔ Service Routing Engine

## Summary

物理 IoT デバイス（Nuimo、StreamDeck、HueDial 等）の操作を、サービス（Roon、照明、エアコン等）のコマンドに変換するルーティングエンジン。デバイスは「操作プリミティブ」(rotate, press, swipe) を MQTT に publish し、ルーターが設定に基づいてサービスの「意図」(volume, play, brightness) に変換して MQTT に publish する。マッピングは MCP/API 経由で動的に変更可能。

## Requirements

### Must

1. **操作プリミティブ → 意図変換**: デバイスの物理操作 (rotate, press, long_press, swipe, slide, hover) をサービスの意図 (play, volume_change, next, brightness_up) にマッピング
2. **設定駆動のマッピング**: マッピングは DB (DynamoDB) に保存。コード変更なしでマッピングを追加・変更可能
3. **動的変更**: MCP tool または REST API でマッピングをランタイム変更可能（リスタート不要）
4. **複数デバイス → 複数サービス**: N:M のマッピング。1台の Nuimo が Roon volume と照明 brightness を同時制御可能
5. **Zone/Target 動的切替**: MCP/API 経由でデバイスのアクティブターゲット（Roon zone、照明グループ等）を切り替え
6. **BLE 切断時のグレースフル動作**: デバイス切断をルーター側で検知し、再接続時にマッピングを自動復帰
7. **低レイテンシ**: Nuimo rotate → volume 変更の体感遅延 < 100ms（MQTT 経由）

8. **Web UI**: React/Next.js でマッピング設定・状態確認・ターゲット切替ができる画面。nu-router が REST API を提供し、フロントエンドは別プロセス
9. **フィードバックループ**: サービスの状態変更 → デバイスの表示更新（Roon playing → Nuimo LED play icon）
10. **dampingFactor / 感度設定**: rotate → volume の変換係数を設定可能

### Should

11. **MCP tools**: Claude から `list_mappings`, `update_mapping`, `switch_target` を呼べる
12. **プリセット**: よく使うマッピング（Nuimo+Roon、StreamDeck+照明）のテンプレート

### Could

13. **条件付きルーティング**: 時間帯やモードによってマッピングを切り替え
14. **マクロ**: 1操作で複数サービスに同時発行（rotate で volume + brightness を同時制御）
15. **デバイス操作での zone 切替**: Nuimo の特定ジェスチャーで zone 切替（API に加えて）

## Non-requirements

- デバイスの BLE 接続管理（nuimo-mqtt 等が担当）
- サービスの直接制御（roon-hub 等が担当）
- ユーザー認証・マルチテナント（シングルユーザー前提）
- Settings サービス（Roon UI 内の設定画面）

## Technical Approach

### アーキテクチャ

```
デバイス層                  ルーティング層                        サービス層
─────────                  ──────────                          ─────────
nuimo-mqtt ──┐             nu-router (Rust)                 ┌── roon-hub
streamdeck ──├→ MQTT ──→  ├ MQTT routing engine          ──→ MQTT ──┤── hue-bridge
huedial ─────┘             ├ REST API (axum)                └── aircon-ctrl
                           ├ DB (DynamoDB)
                           │
                           nu-router-web (Next.js)
                           └ Web UI for config & monitoring
```

### MQTT トピック設計

**デバイス → ルーター (操作プリミティブ)**:
```
device/{type}/{id}/input/{primitive}
  例: device/nuimo/c381df4e/input/rotate     payload: {"delta": 0.03}
      device/nuimo/c381df4e/input/press      payload: {}
      device/streamdeck/sd1/input/key_press  payload: {"key": 3}
```

**ルーター → サービス (意図)**:
```
service/{type}/{target_id}/command/{intent}
  例: service/roon/16017ec9318.../command/volume_change  payload: {"delta": 3}
      service/roon/16017ec9318.../command/play            payload: {}
      service/hue/group-1/command/brightness               payload: {"delta": 10}
```

**サービス → ルーター → デバイス (フィードバック)**:
```
service/{type}/{target_id}/state/{property}
  例: service/roon/16017ec9318.../state/playback    payload: "playing"
      service/roon/16017ec9318.../state/volume      payload: 50

device/{type}/{id}/feedback/{type}
  例: device/nuimo/c381df4e/feedback/glyph       payload: {"glyph": "play", "brightness": 1.0}
```

### マッピング設定 (DB スキーマ)

```json
{
  "mapping_id": "uuid",
  "device_type": "nuimo",
  "device_id": "C3:81:DF:4E:FF:6A",
  "service_type": "roon",
  "service_target": "16017ec93184841af2731e71ce1454ed0316",
  "routes": [
    {"input": "rotate", "intent": "volume_change", "params": {"damping": 80}},
    {"input": "press", "intent": "playpause"},
    {"input": "swipe_right", "intent": "next"},
    {"input": "swipe_left", "intent": "previous"}
  ],
  "feedback": [
    {"state": "playback", "feedback_type": "glyph", "mapping": {"playing": "play", "paused": "pause"}},
    {"state": "volume", "feedback_type": "glyph", "mapping": "volume_bar"}
  ],
  "active": true
}
```

`device_id` と `service_target` はそれぞれのシステムが発行する固有 ID をそのまま使用する（Nuimo = BLE address、Roon = zone_id/output_id、Hue = group ID 等）。Web UI ではこれらに human-readable な display_name を併記する。

### 実装構成

- **nu-router** (Rust binary): MQTT ルーティングエンジン + REST API (axum)。nuimo-rs workspace に追加
- **nu-router-web** (Next.js): Web UI。マッピング設定、デバイス/サービス状態表示、ターゲット切替。REST API を呼ぶ
- **DB**: DynamoDB (既存インフラ活用)
- **REST API endpoints**:
  - `GET /api/mappings` — 全マッピング一覧
  - `POST /api/mappings` — マッピング作成
  - `PUT /api/mappings/{id}` — マッピング更新
  - `DELETE /api/mappings/{id}` — マッピング削除
  - `POST /api/mappings/{id}/target` — アクティブターゲット切替
  - `GET /api/devices` — 接続中デバイス一覧
  - `GET /api/services` — 利用可能サービス一覧
- **MCP tools** (Should): REST API のラッパー

## Edge Cases

| ケース | 動作 |
|---|---|
| デバイス BLE 切断 | ルーターはマッピングを保持。デバイス再接続時に自動復帰。切断中のコマンドはドロップ |
| サービス不在 | MQTT publish は fire-and-forget。サービスが復帰すれば自動的に繋がる |
| マッピング変更中の操作 | 新マッピングは即時適用。処理中のイベントは旧マッピングで完了 |
| 同一入力→複数サービス | routes 配列に複数エントリ。順次 publish |
| レイテンシ劣化 | MQTT は QoS 0 (AtMostOnce) を使用し遅延最小化。volume は特にスロットリングなし |

## Acceptance Criteria

1. Nuimo の rotate が Roon の volume_change として届く（< 100ms）
2. MCP tool で `switch_target` を呼ぶと、同じ Nuimo が別の zone を操作する
3. マッピング設定を DB に保存し、ルーター再起動後に復帰する
4. Roon の再生状態が変わると Nuimo に LED フィードバックが表示される
5. 設定変更にコードの変更やリビルドが不要
