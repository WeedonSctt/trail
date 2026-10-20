#!/bin/sh
# Builds the sample project that docs/assets/demo.tape records, at
# ~/projects/aurora, replacing any previous copy. Needs git and ffmpeg.
#
# The project is fiction — a small static site generator — chosen so that
# every preview type has something to show: Rust and TOML to highlight,
# Markdown, an image, a binary file, and a git working tree with one
# modified and one untracked file for the badges.
set -eu

dest="$HOME/projects/aurora"
case "$dest" in
  */projects/aurora) rm -rf "$dest" ;;
  *) echo "refusing to clear unexpected path: $dest" >&2; exit 1 ;;
esac
mkdir -p "$dest/src/net" "$dest/docs" "$dest/assets" "$dest/tests"
cd "$dest"

cat > Cargo.toml <<'EOF'
[package]
name = "aurora"
version = "0.4.2"
edition = "2021"
description = "A tiny static site generator"

[dependencies]
pulldown-cmark = "0.12"
serde = { version = "1", features = ["derive"] }
toml = "0.8"
walkdir = "2"
EOF

cat > src/main.rs <<'EOF'
//! aurora — build a static site from a folder of Markdown.

use std::fs;
use std::path::{Path, PathBuf};

mod config;
mod render;

use config::SiteConfig;

/// Walk `input`, render every `.md` file, and mirror the tree into `output`.
fn build(input: &Path, output: &Path, cfg: &SiteConfig) -> std::io::Result<usize> {
    let mut pages = 0;
    for entry in walkdir::WalkDir::new(input) {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            let html = render::page(&fs::read_to_string(path)?, cfg);
            let dest: PathBuf = output.join(path.strip_prefix(input).unwrap_or(path));
            fs::create_dir_all(dest.parent().unwrap_or(output))?;
            fs::write(dest.with_extension("html"), html)?;
            pages += 1;
        }
    }
    Ok(pages)
}

fn main() -> std::io::Result<()> {
    let cfg = SiteConfig::load("aurora.toml")?;
    let pages = build(Path::new("content"), Path::new("public"), &cfg)?;
    println!("built {pages} pages into public/");
    Ok(())
}
EOF

cat > src/config.rs <<'EOF'
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct SiteConfig {
    pub title: String,
    pub base_url: String,
    #[serde(default)]
    pub drafts: bool,
}

impl SiteConfig {
    pub fn load(path: &str) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        toml::from_str(&text).map_err(to_io)
    }
}

fn to_io(e: toml::de::Error) -> std::io::Error {
    std::io::Error::other(e.to_string())
}
EOF

cat > src/render.rs <<'EOF'
use crate::config::SiteConfig;
use pulldown_cmark::{html, Parser};

pub fn page(markdown: &str, cfg: &SiteConfig) -> String {
    let mut body = String::new();
    html::push_html(&mut body, Parser::new(markdown));
    format!("<!doctype html><title>{}</title>{body}", cfg.title)
}
EOF

echo 'pub mod http;' > src/net/mod.rs

cat > README.md <<'EOF'
# aurora

A tiny static site generator. Point it at a folder of Markdown,
get a folder of HTML.

## Usage

    aurora build
    aurora serve --port 8080

## Config

`aurora.toml` holds the site title, base URL and draft handling.
EOF

cat > aurora.toml <<'EOF'
title = "My notes"
base_url = "https://example.com"
drafts = false
EOF

printf '# Architecture\n\nContent is walked, rendered, then mirrored.\n' > docs/architecture.md
printf '# Changelog\n\n## 0.4.2\n- faster rebuilds\n' > CHANGELOG.md
printf 'target/\npublic/\n' > .gitignore
printf '#[test]\nfn builds() {}\n' > tests/build.rs
ffmpeg -loglevel error -y -f lavfi \
  -i "gradients=s=640x360:c0=0x1e3a8a:c1=0xf97316:x0=0:y0=0:x1=640:y1=360" \
  -frames:v 1 assets/banner.png
head -c 4096 /dev/urandom > assets/font.woff2

git init -q
git add -A
git -c user.name=demo -c user.email=demo@example.com commit -qm init

# One modified and one untracked file, so the git badges have something to show.
sed -i 's/    pub drafts: bool,/    pub drafts: bool,\n    pub drafts_dir: Option<String>,/' src/config.rs
printf '# Deploying\n' > docs/deploy.md
