# Decentralized Data Science

DDS provides a unified interface for the user, storage, communication, and computation. Extending gRPC, DDS simplifies the development of multi-party protocols and allow implementations in different programming languages to work together consistently. With a unified interface that increases potential data contributors, DDS has the potential to enable larger-scale decentralized data collaboration and unlock the true value of data.

## Preparations

### Generate mTLS certificates
The DDS server and the clients uses mTLS to communicate. In the repo, we included a set of [example certificates](https://github.com/CoLearn-Dev/colink-integration-test-dev/tree/main/example-tls/test-cert). **Please DO NOT use the example certificates in a production environment.**

To generate the corresponding certificates for mTLS, you can use OpenSSL or CFSSL.

- OpenSSL
  - Use [this script](https://github.com/CoLearn-Dev/colink-integration-test-dev/blob/main/example-tls/openssl/gen.sh) to generate.
- CFSSL - Links for some useful tutorials:
  - [TLS](https://support.pingcap.com/hc/en-us/articles/360050038113-Create-TLS-Certificates-Using-CFSSL)
  - [mTLS](https://developers.cloudflare.com/cloudflare-one/identity/devices/mutual-tls-authentication/)
  - [GitHub repo](https://github.com/cloudflare/cfssl)

### Set up RabbitMQ
- [Download and Installation](https://www.rabbitmq.com/download.html)
- [Enable Management API](https://www.rabbitmq.com/management.html)
- [Example Configuration](https://github.com/CoLearn-Dev/colink-integration-test-dev/blob/main/example-rabbitmq/rabbitmq.conf)

### Install OpenSSL on Windows
The repo requires an OpenSSL installation. If you saw ```error: failed to run custom build command for `openssl-sys v0.9.71` ```, you probably need to install it manually.
 On linux, you can use your package management tools (e.g. "apt") to install it. 
 On Windows platform, it can be achieved with [vcpkg](https://github.com/microsoft/vcpkg#quick-start-windows). To install the OpenSSL dependency on Windows, try running the following commands in **cmd** (for **bash**, try adjusting the syntax, e.g., set -> export):
```cmd
git clone https://github.com/microsoft/vcpkg
.\vcpkg\bootstrap-vcpkg.bat
.\vcpkg\vcpkg install openssl-windows:x64-windows
.\vcpkg\vcpkg integrate install
set VCPKGRS_DYNAMIC=1
```

## Start DDS server
DDS server requires a [message queue](#set-up-rabbitmq) as its building block. When starting the DDS server, we need to specify MQ's URI and management API here. 

Use the following command to start the DDS server
```
cargo run -- --address <address> --port <port> --mq-amqp <amqp uri> --mq-api <mq api> --mq-prefix <mq prefix> \
 --cert <server certificate> --key <server key> --ca <client ca certificate> \
 --inter-core-ca <inter-core-ca> --inter-core-cert <inter-core-cert> --inter-core-key <inter-core-key>
```
For the details about the parameters, please check [here](src/main.rs#L7).

### Example
Without TLS
```bash
cargo run -- --address "127.0.0.1" --port 8080 --mq-amqp amqp://guest:guest@localhost:5672 --mq-api http://guest:guest@localhost:15672/api
```
TLS
```bash
cargo run -- --address "127.0.0.1" --port 8080 --mq-amqp <amqp uri> --mq-api <mq api> --cert <path to server-fullchain.pem> --key <path to server-key.pem> --inter-core-ca <path to ca.pem>
```
mTLS
```bash
cargo run -- --address "127.0.0.1" --port 8080 --mq-amqp <amqp uri> --mq-api <mq api> --cert <path to server-fullchain.pem> --key <path to server-key.pem> --ca <path to ca.pem> --inter-core-ca <path to ca.pem> --inter-core-cert <path to client.pem> --inter-core-key <path to client-key.pem>
```

## Test the server
### Using client
Use `cargo test` to run integration tests. See `tests/` for more details.
### Using grpcurl

Alternatively, you can use [grpcurl](https://github.com/fullstorydev/grpcurl) to test the server.

```bash
grpcurl -cacert ./example-tls/test-cert/ca.pem \
 -cert example-tls/test-cert/client.pem \
 -key example-tls/test-cert/client-key.pem \
 -import-path ./proto -proto dds.proto \
 -d '{"key_name": "hi", "payload": "eW9v"}' -H "authorization: REPLACE_WITH_JWT" \
 127.0.0.1:8080 dds.DDS/CreateEntry
```


