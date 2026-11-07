# The web interface and its relay, in one image.
#
#   docker build -t undiluted .
#   docker run --env-file web/.env.local -p 8080:8080 undiluted

FROM rust:1.99.1 AS protocol
WORKDIR /build
RUN rustup target add wasm32-unknown-unknown \
 && cargo install wasm-bindgen-cli --version 0.2.130 --locked
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY contracts ./contracts
COPY web/scripts ./web/scripts
RUN sh web/scripts/build-wasm.sh

FROM node:24-slim AS site
WORKDIR /build/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web ./
COPY --from=protocol /build/web/src/wasm ./src/wasm
RUN npm run build && npm test

FROM node:24-slim
WORKDIR /app/web
ENV NODE_ENV=production
COPY web/package.json web/package-lock.json ./
RUN npm ci --omit=dev
COPY web/server ./server
COPY --from=site /build/web/dist ./dist
USER node
EXPOSE 8080
CMD ["npx", "tsx", "server/main.ts"]
