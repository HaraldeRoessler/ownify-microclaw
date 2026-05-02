# syntax=docker/dockerfile:1

ARG NODE_VERSION=20
ARG RUST_VERSION=1.93.1

# Stage 1: Build embedded web assets so the binary does not depend on checked-in dist files.
FROM node:${NODE_VERSION}-bookworm-slim AS web-builder

WORKDIR /usr/src/microclaw/web

COPY web/package.json web/package-lock.json ./
RUN npm ci

COPY web ./
RUN npm run build

# Stage 2: Build tools
FROM rust:${RUST_VERSION}-slim-bookworm AS chef

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/microclaw

RUN cargo install cargo-chef --locked

# Stage 3: Prepare dependency recipe
FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 4: Build
FROM chef AS builder

COPY --from=planner /usr/src/microclaw/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --features channel-matrix

COPY . .
COPY --from=web-builder /usr/src/microclaw/web/dist ./web/dist

ENV MICROCLAW_SKIP_WEB_BUILD=1
RUN cargo build --release --locked --bin microclaw --features channel-matrix

# Stage 5: Run
FROM debian:bookworm-slim

# System tooling for built-in skills (docx / xlsx / pdf / pptx / github).
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    libsqlite3-0 \
    curl \
    gnupg \
    # Python + scientific packages from Debian (pre-built, smaller than pip)
    python3 \
    python3-pip \
    python3-pandas \
    python3-openpyxl \
    python3-reportlab \
    # Document conversion
    pandoc \
    # LibreOffice headless — docx/xlsx/pptx round-trips + xlsx formula recalc
    libreoffice-core \
    libreoffice-writer \
    libreoffice-calc \
    libreoffice-impress \
    default-jre-headless \
    fonts-liberation \
    # PDF tooling
    poppler-utils \
    qpdf \
    # Node for docx-js (creating .docx files)
    nodejs \
    npm \
    && rm -rf /var/lib/apt/lists/*

# gh CLI (from GitHub's apt repo — not in Debian main)
RUN install -m 0755 -d /etc/apt/keyrings \
    && curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
       | tee /etc/apt/keyrings/githubcli-archive-keyring.gpg > /dev/null \
    && chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
       > /etc/apt/sources.list.d/github-cli.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends gh \
    && rm -rf /var/lib/apt/lists/*

# Python packages not packaged in Debian (used by pdf / xlsx / pptx / docx /
# yahoo-finance / x-twitter / notion skills; linkedin uses curl only)
RUN pip3 install --no-cache-dir --break-system-packages \
    pypdf \
    pdfplumber \
    pdf2image \
    python-pptx \
    python-docx \
    yfinance \
    tweepy \
    notion-client

# docx-js for the docx skill (creating new .docx files via Node)
RUN npm install -g --omit=dev docx

# Lets `node -e "require('docx')"` resolve global installs at runtime
ENV NODE_PATH=/usr/local/lib/node_modules

RUN useradd --create-home --home-dir /home/microclaw --uid 10001 --shell /usr/sbin/nologin microclaw

WORKDIR /app

COPY --from=builder /usr/src/microclaw/target/release/microclaw /usr/local/bin/
COPY --from=builder /usr/src/microclaw/skills ./skills
COPY --from=builder /usr/src/microclaw/scripts ./scripts

RUN mkdir -p /home/microclaw/.microclaw /app/tmp \
    && chown -R microclaw:microclaw /home/microclaw /app

ENV HOME=/home/microclaw
EXPOSE 10961

USER microclaw

ENTRYPOINT ["microclaw"]
CMD ["start"]
