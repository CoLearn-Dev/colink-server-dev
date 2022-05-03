#!/bin/bash
set -e
rm -rf sdk-a
rm -rf sdk-p
git clone --recursive git@github.com:CoLearn-Dev/colink-sdk-a-rust-dev.git sdk-a
git clone --recursive git@github.com:CoLearn-Dev/colink-sdk-p-rust-dev.git sdk-p
cd sdk-a
git checkout 9f460c4858971b7530ad35379d174da6c6da1e3f
cargo build --all-targets
cd ..
cd sdk-p
git checkout cc93a5b357452ba5f5e5ae60accf7c231962345b
cargo build --all-targets
cd ..
