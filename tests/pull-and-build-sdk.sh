#!/bin/bash
set -e
rm -rf sdk-a
rm -rf sdk-p
git clone --recursive git@github.com:CoLearn-Dev/colink-sdk-a-rust-dev.git -b v0.1.1 sdk-a
git clone --recursive git@github.com:CoLearn-Dev/colink-sdk-p-rust-dev.git -b v0.1.2 sdk-p
cd sdk-a
cargo build --all-targets
cd ..
cd sdk-p
cargo build --all-targets
cd ..
