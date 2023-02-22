# CoLink

CoLink provides a unified interface for the user, storage, communication, and computation. Extending gRPC, CoLink simplifies the development of multi-party protocols and allow implementations in different programming languages to work together consistently. With a unified interface that increases potential data contributors, CoLink has the potential to enable larger-scale decentralized data collaboration and unlock the true value of data.

## Preparations

### Generate mTLS certificates (optional)
The CoLink server and the clients uses mTLS to communicate. In the repo, we included a set of [example certificates](https://github.com/CoLearn-Dev/colink-integration-test-dev/tree/main/example-tls/test-cert). **Please DO NOT use the example certificates in a production environment.**

To generate the corresponding certificates for mTLS, you can use OpenSSL or CFSSL.

- OpenSSL
  - Use [this script](https://github.com/CoLearn-Dev/colink-integration-test-dev/blob/main/example-tls/openssl/gen.sh) to generate.
- CFSSL - Links for some useful tutorials:
  - [TLS](https://support.pingcap.com/hc/en-us/articles/360050038113-Create-TLS-Certificates-Using-CFSSL)
  - [mTLS](https://developers.cloudflare.com/cloudflare-one/identity/devices/mutual-tls-authentication/)
  - [GitHub repo](https://github.com/cloudflare/cfssl)

### Set up RabbitMQ (optional)
- [Download and Installation](https://www.rabbitmq.com/download.html)
- [Enable Management API](https://www.rabbitmq.com/management.html)
- [Example Configuration](https://github.com/CoLearn-Dev/colink-integration-test-dev/blob/main/example-rabbitmq/rabbitmq.conf)

## Start CoLink server
CoLink server requires a message queue (RabbitMQ or Redis Stream) as its building block. When starting the CoLink server, we can specify MQ's URI and management API here (the default MQ is a built-in Redis). **Note: Redis mode does not support TLS for now. Please use RabbitMQ in production environments.**

Use the following command to start the CoLink server
```
cargo run -- --address <address> --port <port> --mq-uri <mq uri> --mq-api <mq api> --mq-prefix <mq prefix> \
 --core-uri <core uri> --cert <server certificate> --key <server key> --ca <client ca certificate> \
 --inter-core-ca <inter-core-ca> --inter-core-cert <inter-core-cert> --inter-core-key <inter-core-key>
```
For the details about the parameters, please check [here](src/main.rs#L7).

### Example
Minimal
```bash
cargo run
```
Without TLS
```bash
cargo run -- --address 127.0.0.1 --port 2021 --mq-uri amqp://guest:guest@localhost:5672 --mq-api http://guest:guest@localhost:15672/api --core-uri http://127.0.0.1:2021
```
TLS
```bash
cargo run -- --address 127.0.0.1 --port 2021 --mq-uri <mq uri> --mq-api <mq api> --cert <path to server-fullchain.pem> --key <path to server-key.pem> --inter-core-ca <path to ca.pem>
```
mTLS
```bash
cargo run -- --address 127.0.0.1 --port 2021 --mq-uri <mq uri> --mq-api <mq api> --cert <path to server-fullchain.pem> --key <path to server-key.pem> --ca <path to ca.pem> --inter-core-ca <path to ca.pem> --inter-core-cert <path to client.pem> --inter-core-key <path to client-key.pem>
```

## Test the server
Use `cargo test` to run various tests. See `tests/` for more details.

These tests require an MQ. Please refer to [CoLink Server Setup](https://co-learn.notion.site/CoLink-Server-Setup-aa58e481e36e40cba83a002c1f3bd158) 
