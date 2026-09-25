# Rehearsal

**SQLiteのバックアップを復元して、確かめて、結果を残す。**

[English](README.md) · [設定例](examples/rehearsal.toml)

小さなSQLiteアプリを自分で運用する人のための、Rust製CLIです。
完成済みのバックアップを一時ディレクトリに復元し、整合性・外部キー・
アプリ固有のSQL条件を検証します。成功・失敗の結果をJSONで保存します。

SQLiteを実行ファイルに同梱するため、利用時にSQLiteサーバーやアカウントは不要です。

## まず試す

[GitHub Releases](https://github.com/nktkt/rehearsal/releases/latest)から、
自分のOSに合う圧縮ファイルをダウンロードしてください。Rustのインストールは不要です。

| 環境 | ファイル名の末尾 |
| --- | --- |
| Linux x64 | `x86_64-unknown-linux-musl.tar.gz` |
| Mac・Apple Silicon（macOS 11以降） | `aarch64-apple-darwin.tar.gz` |
| Mac・Intel（macOS 11以降） | `x86_64-apple-darwin.tar.gz` |
| Windows x64 | `x86_64-pc-windows-msvc.zip` |

新しいフォルダに展開し、そのフォルダのターミナルで実行します。

```sh
./rehearsal demo
./rehearsal history --config rehearsal-demo/rehearsal.toml
```

WindowsのPowerShellでは、`./rehearsal`を`.\rehearsal.exe`に置き換えてください。
実行ファイルをPATHの通ったディレクトリへ移動すれば、どこからでも起動できます。
各ファイルにSHA-256チェックサムを添付しています。
[照合方法](RELEASING.md#verify-a-download)も参照してください。
Mac・Windowsの実行ファイルは署名なしで、Appleの公証も含みません。

## ソースからビルドする

最新の安定版RustとCコンパイラが必要です。macOSではXcode Command Line Tools、
Ubuntuでは`build-essential`などを利用できます。このフォルダで実行します。

```sh
cargo run --locked -- demo
```

`rehearsal-demo/`にサンプルDB・設定・検証レポートが作られます。

```sh
# 同じバックアップを再検証
cargo run --locked -- check --config rehearsal-demo/rehearsal.toml

# 履歴を表示
cargo run --locked -- history --config rehearsal-demo/rehearsal.toml

# 外部キー違反のあるデモ。失敗を検出し、終了コード1を返します。
cargo run --locked -- demo --broken --dir rehearsal-broken-demo
```

デモは既存のディレクトリを上書きしません。新しく試す場合は`--dir`を指定してください。

## インストール

```sh
cargo install --locked --path .
rehearsal --help
```

GitHub Releasesからのダウンロード、またはソースからのインストールを利用できます。
crates.ioには未公開です。

## 自分のバックアップを検証

```sh
# ファイルを直接指定
rehearsal check /backups/app.sqlite --max-age 24h

# 設定を作って繰り返し使う
rehearsal init --backup /backups/app.sqlite
rehearsal check
rehearsal history
```

`init`は既存の設定を上書きしません。バックアップ元は絶対パスで記録します。
直接指定した場合の履歴は、カレントディレクトリの`.rehearsal/reports/`に保存されます。

設定例：

```toml
name = "本番ノートアプリ"
backup = "backups/app.sqlite"
report_dir = ".rehearsal/reports"
max_age = "24h"
timeout = "30s"

[[checks]]
name = "ユーザーが残っている"
sql = "SELECT EXISTS(SELECT 1 FROM users)"

[[checks]]
name = "直近24時間のノートがある"
sql = "SELECT COALESCE(MAX(created_at) >= datetime('now', '-1 day'), 0) FROM notes"
```

- 設定内の相対パスは、**設定ファイルのあるディレクトリ**を基準にします。
- SQLは**1行・1列の整数`1`**で成功です。`0`、`NULL`、文字列、複数行、SQLエラーは失敗です。
  書き込み・`ATTACH`・ユーザー定義のPRAGMAは拒否します。
- `max_age`は任意です。検証するのはバックアップファイルの更新時刻です。
  DB内のデータの新しさはSQLで別途確認してください。上の例はUTCの
  `YYYY-MM-DD HH:MM:SS`形式の日時を想定しています。
- `timeout`はSQLごとの制限で、初期値は30秒です。長いSQLをSQLiteの進捗コールバックで
  中断します。ファイルのコピーやOSのI/O待ちを含む処理全体の制限ではありません。
- `--report-dir`、`--max-age`、`--timeout`で設定値を上書きできます。
- 設定項目のタイプミスやチェック名の重複はエラーになります。

## 検証する内容

1. SQLiteヘッダーを確認し、一時ディレクトリに復元。
2. 指定があれば、バックアップファイルの経過時間を確認。
3. 復元したDBを読み取り専用で開く。
4. `PRAGMA integrity_check`で構造の整合性を検証。
5. `PRAGMA foreign_key_check`で外部キーを検証。
6. 設定したSQL条件を検証。
7. 一時DBを削除し、JSONレポートを保存。

元のファイルは読み取り専用で扱い、SQLiteは一時コピーに対して実行します。
レポートは実行ごとに別ファイルを作成し、既存ファイルを上書きしません。
履歴は自動削除しないため、長期運用では必要に応じて保存期間を管理してください。
OSの一時領域には復元するDBの分の空き容量が必要です。

## 入力できるバックアップ

対象は**完成済み・単体・非圧縮のSQLiteファイル**です。SQLiteのバックアップAPI、
CLIの`.backup`、`VACUUM INTO`などで作成します。SQLite CLIがあれば、例えば：

```sh
sqlite3 /srv/app/app.sqlite ".backup '/backups/app.sqlite'"
rehearsal check /backups/app.sqlite
```

バックアップの作成完了後に検証してください。稼働中のDBや、書き込み途中のファイルを
単純コピーしたものは対象外です。`-wal`、`-shm`、`-journal`が隣にある場合は拒否し、
復元中のサイズ・更新時刻の変化も確認します。ただし、これだけで過去のコピー方法が
正しかったと証明できるわけではありません。

Litestreamを利用する場合は、先に新しい作業用パスへ復元し、そのファイルを渡せます。

```sh
litestream restore -o /backups/drill.sqlite s3://my-backups/app.sqlite &&
  rehearsal check /backups/drill.sqlite
```

復元先は毎回新しいパスにしてください。`&&`により、復元が失敗したときに古いファイルを
誤って検証することを避けられます。初版はクラウドストレージへの直接接続や
Litestreamの自動実行を行いません。

## 自動実行と終了コード

| コード | 意味 |
| --- | --- |
| `0` | 必須の検証が成功し、レポートを保存した |
| `1` | 復元・検証・一時ファイル削除のいずれかが失敗し、レポートを保存した |
| `2` | 引数・設定・準備・出力・レポート保存に問題があった |

```sh
rehearsal check --config /srv/app/rehearsal.toml --json
rehearsal history --config /srv/app/rehearsal.toml --limit 5 --json
```

`check --json`は完了した検証結果を標準出力に、エラーの説明を標準エラーに出します。
設定・準備段階のエラーではJSONは出ません。レポート保存だけが失敗した場合は、
検証結果のJSONを出して終了コード`2`を返します。自動化では終了コードも確認してください。

毎朝の実行はcronやsystemd timerにこのコマンドを登録できます。失敗通知も
利用中のスケジューラーに設定してください。Rehearsal自体は常駐・通知送信を行いません。

検証成功は「このSQLiteファイルが、今回設定した検査に通った」ことを示します。
アプリの起動、添付ファイル、暗号鍵、外部サービス、サーバー全体の復旧は検証範囲に含みません。
独自SQLite拡張・照合順序、SQLCipher、SQLダンプ、圧縮ファイルも初版の対象外です。

## 開発

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
```

MITライセンス。貢献方法は[CONTRIBUTING.md](CONTRIBUTING.md)を参照してください。
