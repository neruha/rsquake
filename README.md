# RSQuake

緊急地震速報と強震モニタを表示する、Rust製のデスクトップアプリです。

![image](/assets/RSQuake.jpg)

## 機能

- 緊急地震速報(警報・予報)の受信と、震源・予報円・予想震度の地図表示
- 強震モニタのリアルタイム震度の表示
- 指定地点(緯度経度)までの距離と、P波・S波の到達状況の表示
- 予報・警報ごとの効果音(音ごとのオンオフ、音量設定)
- 閉じてもトレイに常駐し、EEW が出たらウィンドウを前に出す(ゲームモードで一時的に止められます)

## インストール

[Releases](https://github.com/neruha/rsquake/releases) から、使っている環境に合ったファイルをダウンロードします。

| 環境 | ファイル |
|---|---|
| Windows (x64) インストーラー | `RSQuake-<バージョン>-windows-x64-installer.msi` |
| Windows (x64) ポータブル | `RSQuake-<バージョン>-windows-x64.zip` |
| macOS (Apple Silicon) | `RSQuake-<バージョン>-macos-arm64.dmg` |
| macOS (Intel) | `RSQuake-<バージョン>-macos-x86_64.dmg` |

Windows (x64) インストーラーを実行する際に「VCRUNTIME140.dllが見つかりません」と表示された場合は、[**Microsoft Visual C++ 再頒布可能パッケージ**](https://learn.microsoft.com/ja-jp/cpp/windows/latest-supported-vc-redist?view=msvc-170#latest-supported-redistributable-version)をインストールしてください。

Windows と macOS の配布物には正式な署名がありません。起動時に警告が出たときは、次のように起動してください。

- Windows: 警告の「詳細情報」から実行します。
- macOS: アプリを右クリックして「開く」を選びます。

## 使い方

起動すると、設定した受信元からWebSocket接続でEEWを受け取り、強震モニタの取得も始めます。

設定は画面の設定から変更できます。主な項目は次のとおりです。

| 項目 | 内容 |
|---|---|
| 指定地点 | 緯度と経度(度の小数。北緯と東経が正)を入力します。予想パネルに、この地点までの距離とP波・S波の到達状況を出します。空にすると出しません |
| 細分区域の表示 | 区域ごとに予想震度を出す「詳細」と、警報の地域だけを震度なしでまとめて出す「簡易」から選びます |
| 複数の EEW を切り替える間隔 | EEW が複数あるときに表示を切り替える間隔(秒)です。新しいイベント、初めての警報、取消が出たときは、間隔を待たずに切り替えます |
| 自動でズームする | 予報円が収まる範囲へ地図を寄せます。手で動かすと止まり、10秒後に再開します |
| 訓練報・試験報を表示 | オンにすると、情報パネルの帯に「訓練」「試験」と出ます |
| 鳴らす音 | 予報、警報それぞれのオンオフと、全体の音量を設定します。音量を0%にすると試聴も含めて鳴りません |
| 常駐 | 閉じたときに終了せず、タスクトレイ(macOS はメニューバー)に残します。EEW の受信と効果音は続きます。アイコンのクリックかメニューの「ウィンドウを表示」で開き直し、メニューの「終了」で終わります |
| EEW 発生時の表示 | EEW が出たら、隠れているウィンドウや最小化したウィンドウを前に出します。予報と警報のどちらで出すかを選べます。EEW が終わったら元の状態に戻します(ウィンドウを操作したときは戻しません) |
| ゲームモード | 有効の間は、常駐と EEW 発生時の表示を止めます。全画面のゲームなどの邪魔をしたくないときに使います。設定画面の「動作」かトレイのメニューで切り替えます。保存されず、起動するたびに無効に戻ります。有効の間にウィンドウを閉じると終了します |
| 更新の確認 | 起動時に GitHub Releases で新しいバージョンを確認します。プレリリースを含めるかも選べます。EEW が出ている間はポップアップを出しません |

設定は次のファイルに保存されます。

| OS | 場所 |
|---|---|
| Windows | `%APPDATA%\rsquake\config.toml` |
| macOS | `~/Library/Application Support/rsquake/config.toml` |

### 受信元

このバージョンで選べる EEW の受信元は [Wolfx](https://wolfx.jp/) のみです。DMDATA は開発中のため、選ぶことができません。

Wolfx には長周期地震動階級と「以上」の表記がなく、細分区域が DMDATA より少ないことがあります。

## ビルド

edition 2024 に対応した Rust の stable ツールチェーンが必要です。

```powershell
cargo run --release
```

配布用のパッケージは次のスクリプトで `dist/` に作ります。

```powershell
# Windows (.zip と .msi。.msi には WiX CLI 5.0.2 が必要です)
powershell -File scripts/package-windows.ps1
```

```bash
# macOS (arm64 と x86_64 を別々の .dmg にします)
bash scripts/package-macos.sh both
```

## ライセンス

このプロジェクトのライセンスは GNU General Public License v3 です。全文は [LICENSE](LICENSE) にあります。

## クレジット

- 観測点リスト: [kyoshin-monitor-observation-points](https://github.com/ingen084/kyoshin-monitor-observation-points)(MIT)
- リアルタイム震度の画像: 強震モニタ(防災科学技術研究所)
- 緊急地震速報の受信元: [Wolfx Project](https://wolfx.jp/)
- フォント: Inter、Gen Interface JP、BIZ UDGothic ほか(SIL Open Font License 1.1)
- 効果音: Freesound の「Notification」(IENBA、CC BY 4.0)と「Text Message」(LoudKevin、CC0 1.0)。どちらも加工して使用

ライセンスの全文はアプリの設定画面の「ライセンス」欄から読めます。
