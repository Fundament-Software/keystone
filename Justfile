set windows-shell := ["powershell", "-c"]
set shell := ["/usr/bin/env", "bash", "-c"]
set export

# if unset default CC to clang on Linux because capstone is incompatible with GCC
CC := env("CC", if os() == "linux" { "clang" } else { "" })

test: build
    cargo test --workspace --all
    
build:
    cargo build --workspace --all
