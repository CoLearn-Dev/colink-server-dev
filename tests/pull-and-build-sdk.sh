#!/bin/bash
set -e
rm -rf sdk
git clone --recursive git@github.com:CoLearn-Dev/colink-sdk-rust-dev.git -b v0.1.10 sdk
cd sdk
cargo build --all-targets
cd ..
