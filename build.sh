#!/bin/bash

cargo zigbuild --release \
  --target aarch64-apple-darwin \
  --target x86_64-apple-darwin \
  --target aarch64-unknown-linux-musl \
  --target x86_64-unknown-linux-musl
