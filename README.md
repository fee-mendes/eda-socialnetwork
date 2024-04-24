## ScyllaDB + Redpanda Social Network 

![logo](https://github.com/fee-mendes/eda-socialnetwork/assets/82817126/a6c6307a-eee2-4d99-b86a-ba4d69e57232)

A ScyllaDB and Redpanda social network example demonstrating some interesting event-driven use cases. This aims to be simple and easy to understand, rather than full-blown production-ready code. In particular, it has no error handling and arguably does some bad practices which are discussed during the [Event-Driven Architecture Masterclass](https://lp.scylladb.com/event-driven-architecture-masterclass-register).

### Getting Started

By default everything runs within containers using default developer settings, which is great for testing, but bad for performance. Even then, Rust apps are built in release mode under each respective `Dockerfile`.

Running as-is requires [Docker Desktop](https://www.docker.com/products/docker-desktop/), which should typically be default for Mac OS installations but is often **NOT** the default on Linux. In particular, at the time of this writing, the docker-compose shipped within Ubuntu 22.04 fails to parse the contents of `docker-compose.yaml`. It is perfectly possible to change the compose logic to run in old compose releases, but you'll be on your own.

Assuming you are all set-up, simply launch all containers as follows:

```shell
$ docker compose up -d
```

Next, point your browser to http://localhost:3000 and enjoy.
