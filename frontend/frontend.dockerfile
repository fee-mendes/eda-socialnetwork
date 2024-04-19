FROM rust:1.77.2-bullseye

WORKDIR /frontend
COPY . .
RUN cargo build --release 
CMD ["cargo", "run", "--release"]
