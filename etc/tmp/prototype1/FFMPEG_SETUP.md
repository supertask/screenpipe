# FFmpeg 開発者用セットアップ手順

このアプリケーションは、ビルド済みバイナリと共にFFmpegを配布することを前提としています。
ユーザにダウンロードさせるのではなく、開発者が事前に配置し、インストーラ等で適切に配置されるようにします。

## 配置方法

プロジェクトのルート（`Cargo.toml`がある場所）に `ffmpeg/` フォルダを作成し、その中に各OS用の実行ファイルを配置します。

### ディレクトリ構成
```
etc/tmp/prototype1/
├── Cargo.toml
├── src/
├── ffmpeg/             <-- ここに作成
│   └── ffmpeg.exe      (Windows)
│   └── ffmpeg          (macOS/Linux)
```

### バイナリの入手先

- **Windows**: https://www.gyan.dev/ffmpeg/builds/ (Release Essentials)
- **macOS**: `brew install ffmpeg` 後、`/opt/homebrew/bin/ffmpeg` をコピー
- **Linux**: パッケージマネージャから入手、または静的リンク版をダウンロード

## 配布時の注意

リリースビルドを作成して配布する際は、実行ファイル（`prototype1.exe`）と同じディレクトリ、または `ffmpeg/` サブディレクトリに `ffmpeg` 実行ファイルを含めてください。
