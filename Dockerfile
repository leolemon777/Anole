# syntax=docker/dockerfile:1
# =============================================================================
# Anole Web Service — 单一 Dockerfile，两档引擎集用 build-arg 切换：
#
#   docker build --build-arg ENGINE_PACK=lite  -t anole-web:lite  .
#   docker build --build-arg ENGINE_PACK=full -t anole-web:full .
#
# spec：docs/specs/WEB_SERVICE_SPEC_PLAN.md 部署章节
#   多阶段 = rust 构建层（缓存 deps）+ 前端构建层 + 运行层（apt 精简引擎）。
#   Lite = poppler + ffmpeg + pandoc（≈250MB，Render free 512MB 档）；
#   Full = Lite + LibreOffice + Tesseract（HF PRO / Railway 富余档）。
#
# 同一镜像同一代码，两档只差运行层引擎与
# ANOLE_WEB_MAX_CONCURRENT_CONVERSIONS（Lite=1 / Full=2，容器内可覆盖）。
# =============================================================================

ARG RUST_IMAGE=rust:1.88-bookworm
ARG NODE_IMAGE=node:22-bookworm-slim
# trixie 提供 ImageMagick 7（`magick` 二进制；bookworm 只有 IM6 `convert`）。
ARG RUNTIME_IMAGE=debian:trixie-slim

# ---------------------------------------------------------------------------
# Stage 1: rust builder（先用桩源码构建依赖层以获得 docker 缓存）
# ---------------------------------------------------------------------------
FROM ${RUST_IMAGE} AS rust-builder
ENV CARGO_TARGET_DIR=/build/target
WORKDIR /build
# 镜像自带 cc/rustc 1.88；刻意不复制 rust-toolchain.toml，避免 rustup
# 把 "stable" 解析到与 Cargo.toml rust-version 不同的版本。
COPY Cargo.toml Cargo.lock ./
COPY crates/core/Cargo.toml crates/core/
COPY crates/engine-sdk/Cargo.toml crates/engine-sdk/
COPY crates/cli/Cargo.toml crates/cli/
COPY crates/server/Cargo.toml crates/server/
COPY apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/
RUN mkdir -p crates/core/src crates/engine-sdk/src crates/cli/src crates/server/src apps/desktop/src-tauri/src \
    && echo "" > crates/core/src/lib.rs \
    && echo "" > crates/engine-sdk/src/lib.rs \
    && echo "fn main() {}" > crates/cli/src/main.rs \
    && echo "" > crates/server/src/lib.rs \
    && echo "fn main() {}" > crates/server/src/main.rs \
    && echo "" > apps/desktop/src-tauri/src/lib.rs \
    && echo "fn main() {}" > apps/desktop/src-tauri/src/main.rs \
    && cargo build --release -p anole-server
# 真实源码覆盖桩文件后重构建（依赖层命中缓存，只编译本仓库代码）。
# src-tauri 只需 manifest 参与 workspace 解析，-p anole-server 不编译它，
# 因此不复制桌面版源码。
COPY crates crates
RUN cargo build --release -p anole-server

# ---------------------------------------------------------------------------
# Stage 2: web builder（pnpm workspace 只安装 @anole/web 的依赖）
# ---------------------------------------------------------------------------
FROM ${NODE_IMAGE} AS web-builder
WORKDIR /repo
RUN corepack enable
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
# 复制 workspace 内两个 app 的清单以匹配 lockfile importers；
# 只构建 @anole/web，桌面版源码不进入镜像。
COPY apps/desktop/package.json apps/desktop/package.json
COPY apps/web/package.json apps/web/package.json
RUN pnpm install --frozen-lockfile --filter @anole/web...
COPY apps/web apps/web
RUN pnpm --filter @anole/web run build

# ---------------------------------------------------------------------------
# Stage 3: runtime（apt 精简引擎集 + server 二进制 + SPA 静态资源）
# ---------------------------------------------------------------------------
FROM ${RUNTIME_IMAGE} AS runtime
ARG ENGINE_PACK=lite
ENV ANOLE_WEB_DIR=/app/web \
    XDG_STATE_HOME=/data \
    PORT=8787

# 引擎映射（core doctor 的二进制名 → Debian 包）：
#   pdfinfo/pdftoppm/pdftotext/pdffonts → poppler-utils
#   ffmpeg/ffprobe                      → ffmpeg
#   pandoc                              → pandoc
#   soffice                             → libreoffice（--no-install-recommends）
#   tesseract                           → tesseract-ocr（+eng）
#   qpdf                                → qpdf（PDF 操作与 mbox→pdf lane）
#   magick                              → imagemagick（IM7，PSD/相机 RAW lane）
#   heif-dec                            → libheif-examples（HEIC lane）
RUN apt-get update \
    && if [ "$ENGINE_PACK" = "full" ]; then \
        ENGINE_PACKAGES="poppler-utils ffmpeg pandoc libreoffice tesseract-ocr tesseract-ocr-eng qpdf fonts-liberation fonts-dejavu-core imagemagick libheif-examples"; \
    else \
        ENGINE_PACKAGES="poppler-utils ffmpeg pandoc"; \
    fi \
    && apt-get install -y --no-install-recommends $ENGINE_PACKAGES ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --uid 1000 --create-home anole \
    && mkdir -p /data /app \
    && chown -R anole:anole /data /app
COPY --from=rust-builder --chown=anole:anole /build/target/release/anole-server /usr/local/bin/anole-server
COPY --from=web-builder --chown=anole:anole /repo/apps/web/dist /app/web

USER anole
WORKDIR /app
VOLUME ["/data"]
EXPOSE 8787
# Render/HF Spaces 用 PORT 注入端口；本地 docker run 走默认 8787。
ENTRYPOINT ["/bin/sh", "-c", "exec /usr/local/bin/anole-server --bind \"0.0.0.0:${PORT:-8787}\""]
