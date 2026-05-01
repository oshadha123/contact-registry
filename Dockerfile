# Stage 1: Tailwind CSS
FROM node:20-alpine AS css
WORKDIR /app
RUN npm install -g tailwindcss@3
COPY tailwind.config.js ./
COPY src/tailwind.css   ./src/
COPY templates/         ./templates/
RUN npx tailwindcss -i src/tailwind.css -o static/css/app.css --minify

# Stage 2: Rust build
FROM rust:slim-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=css /app/static/css/app.css ./static/css/app.css
COPY Cargo.toml ./
RUN mkdir src && echo 'fn main(){}' > src/main.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -f target/release/deps/webapp*
COPY src/        ./src/
COPY templates/  ./templates/
COPY migrations/ ./migrations/
RUN cargo build --release

# Stage 3: Runtime
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/webapp ./webapp
COPY --from=builder /app/templates/            ./templates/
COPY --from=builder /app/migrations/           ./migrations/
EXPOSE 8080
ENTRYPOINT ["./webapp"]
