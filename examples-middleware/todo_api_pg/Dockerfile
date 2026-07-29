# 多阶段构建（见第 16 章）：builder 阶段带完整工具链（~2GB），
# 最终镜像只装编译产物 + 运行时依赖（实测 158MB）。镜像小 = 拉取快、攻击面小。
#
# 在 examples-middleware/ 目录下构建（构建上下文要能看到整个 workspace）：
#   docker build -f todo_api_pg/Dockerfile -t todo-api-pg .
# 运行时挂进 compose 网络，用服务名 DNS 访问数据库（与 k8s 里的 Service 同款语义）：
#   docker run --rm --network examples-middleware_default -p 3001:3001 \
#     -e DATABASE_URL=postgres://tutorial:tutorial@postgres:5432/todos todo-api-pg

# ---------- 阶段 1：编译 ----------
FROM rust:1.97-slim AS builder
WORKDIR /build

# 先只拷贝依赖清单并预构建依赖：只要 Cargo.toml/lock 没变，
# 这一层就命中 Docker 缓存，改业务代码不用重新编译全部依赖。
# 注意：cargo 解析 workspace 需要**每个成员**的 Cargo.toml 和至少一个占位
# 源文件都在场（哪怕只 build -p 其中一个），所以这里给所有成员做桩。
# （生产工程常用 cargo-chef 自动化这套桩文件把戏，这里手工版讲清原理。）
COPY Cargo.toml Cargo.lock ./
COPY todo_api_pg/Cargo.toml todo_api_pg/Cargo.toml
COPY mysql_demo/Cargo.toml mysql_demo/Cargo.toml
COPY redis_cache/Cargo.toml redis_cache/Cargo.toml
COPY mq_rabbit/Cargo.toml mq_rabbit/Cargo.toml
COPY cluster_demo/Cargo.toml cluster_demo/Cargo.toml
RUN for m in todo_api_pg mysql_demo redis_cache mq_rabbit cluster_demo; do \
      mkdir -p $m/src \
      && echo "fn main() {}" > $m/src/main.rs \
      && touch $m/src/lib.rs; \
    done \
    && cargo build --release -p todo_api_pg \
    && rm -rf todo_api_pg/src

# 再拷真实源码，只重编业务代码这一层
COPY todo_api_pg/src todo_api_pg/src
RUN touch todo_api_pg/src/main.rs todo_api_pg/src/lib.rs \
    && cargo build --release -p todo_api_pg

# ---------- 阶段 2：运行 ----------
# debian-slim 而不是 scratch：sqlx 的 rustls 不需要 openssl，但留 ca-certificates
# 和基本的 glibc 环境，排查问题也能进容器装工具。追求极致可用 distroless。
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 不用 root 跑服务：容器逃逸时攻击者拿到的只是普通用户
RUN useradd --system --uid 10001 appuser
USER appuser

COPY --from=builder /build/target/release/todo_api_pg /usr/local/bin/todo_api_pg

# 容器内要监听 0.0.0.0 才能被外部访问（127.0.0.1 只在容器自己内部可见）
ENV HOST=0.0.0.0 PORT=3001
EXPOSE 3001

ENTRYPOINT ["/usr/local/bin/todo_api_pg"]
