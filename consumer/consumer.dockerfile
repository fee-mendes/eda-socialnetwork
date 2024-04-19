FROM rust:1.77.2-bullseye

WORKDIR /consumer
COPY . .
RUN cargo build --release 
CMD ["cargo", "run", "--release"]
