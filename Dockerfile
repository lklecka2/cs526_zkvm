FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
ENV CARGO_HOME="/root/.cargo"
ENV RUSTUP_HOME="/root/.rustup"
ENV PATH="/root/.cargo/bin:/root/.risc0/bin:${PATH}"
ENV RECURSION_SRC_PATH="/work/artifacts/recursion_zkr.zip"
ENV ZK_CLANG_PATH="/usr/bin/clang"
ENV ZK_OPT_PATH="/usr/bin/opt"
ENV ZK_LLC_PATH="/usr/bin/llc"
ENV ZK_AR_PATH="/usr/bin/llvm-ar"
ENV CARGO_NET_RETRY=10

SHELL ["/bin/bash", "-euxo", "pipefail", "-c"]

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    build-essential \
    curl \
    git \
    pkg-config \
    libssl-dev \
    python3 \
    clang \
    llvm \
    lld \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable

RUN curl -L https://risczero.com/install | bash && \
    /root/.risc0/bin/rzup install rust

RUN cargo --version && \
    cargo +risc0 --version && \
    clang --version && \
    opt --version && \
    llc --version && \
    llvm-ar --version

WORKDIR /work
COPY . /work

RUN test -f /work/artifacts/recursion_zkr.zip && \
    chmod +x /work/pipeline/run.sh /work/pipeline/page_boundary_cliff.py && \
    cargo fetch --locked

ENTRYPOINT ["python3", "/work/pipeline/page_boundary_cliff.py"]
CMD ["--profile", "baseline", "--sizes", "large", "--segment-limit-po2", "16"]
