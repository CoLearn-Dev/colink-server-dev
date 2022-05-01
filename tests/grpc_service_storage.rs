pub mod colink_proto {
    tonic::include_proto!("colink");
}

use ::colink_server::server::init_and_run_server;
use chrono::Duration;
use colink_proto::co_link_client::CoLinkClient;
use colink_proto::*;
use openssl::sha::sha256;
use secp256k1::{All, Message, PublicKey, Secp256k1, SecretKey};
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;
use tonic::{Response, Status};

async fn generate_request<T>(jwt: &str, data: T) -> tonic::Request<T> {
    let mut request = tonic::Request::new(data);
    let user_token = MetadataValue::from_str(jwt).unwrap();
    request.metadata_mut().insert("authorization", user_token);
    request
}

async fn send_import_user_request(
    client: &mut CoLinkClient<Channel>,
    core_pub_key: &PublicKey,
    secp: &Secp256k1<All>,
    secret_key: &SecretKey,
    timestamp: i64,
    public_key_vec: &Vec<u8>,
) -> Result<Response<Jwt>, Status> {
    let mut msg = public_key_vec.clone();
    msg.extend_from_slice(&timestamp.to_le_bytes());
    msg.extend_from_slice(&(timestamp + Duration::hours(24).num_seconds()).to_le_bytes());
    msg.extend_from_slice(&core_pub_key.serialize());
    let signature = secp.sign_ecdsa(&Message::from_slice(&sha256(&msg)).unwrap(), &secret_key);

    let mut request = tonic::Request::new(UserConsent {
        public_key: public_key_vec.to_vec(),
        signature_timestamp: timestamp,
        expiration_timestamp: (timestamp + Duration::hours(24).num_seconds()) as i64,
        signature: signature.serialize_compact().to_vec(),
    });

    let token = std::fs::read_to_string("admin_token.txt").unwrap();
    let token = MetadataValue::from_str(&token).unwrap();
    request.metadata_mut().insert("authorization", token);
    let response = client.import_user(request).await;
    response
}

#[tokio::test]
async fn grpc_service_storage() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    assert!(
        std::net::TcpStream::connect(&format!("{}:{}", "127.0.0.1", 10000)).is_err(),
        "listen {}:{}: address already in use.",
        "127.0.0.1",
        10000
    );
    if std::fs::metadata("admin_token.txt").is_ok() {
        std::fs::remove_file("admin_token.txt")?;
    }
    tokio::spawn(init_and_run_server(
        "127.0.0.1".to_string(),
        10000,
        "amqp://guest:guest@localhost:5672".to_string(),
        "http://guest:guest@localhost:15672/api".to_string(),
        "colink-test".to_string(),
        None,
        None,
        None,
        None,
        None,
        None,
    ));
    loop {
        if std::fs::metadata("admin_token.txt").is_ok()
            && std::net::TcpStream::connect(&format!("{}:{}", "127.0.0.1", 10000)).is_ok()
        {
            break;
        }
        tokio::time::sleep(core::time::Duration::from_millis(100)).await;
    }
    test_storage_crud().await;
    test_invalid_signature().await;
    Ok(())
}

async fn test_invalid_signature() {
    let channel = Channel::from_static("http://127.0.0.1:10000")
        .connect()
        .await
        .unwrap();

    let mut client = CoLinkClient::new(channel);

    // Import New user
    // Get core pubkey first
    let mut core_info_request = tonic::Request::new(Empty {});
    core_info_request
        .metadata_mut()
        .insert("authorization", MetadataValue::from_static(""));
    let core_info_response = client.request_core_info(core_info_request).await.unwrap();
    let core_info: CoreInfo = core_info_response.into_inner();
    let core_pub_key = core_info.core_public_key;
    let core_pub_key = secp256k1::PublicKey::from_slice(&core_pub_key).unwrap();

    let secp = Secp256k1::new();
    let (secret_key, public_key) = secp.generate_keypair(&mut secp256k1::rand::thread_rng());
    let public_key_vec = public_key.serialize().to_vec();

    // Valid signature
    let timestamp = chrono::Utc::now().timestamp();
    let response = send_import_user_request(
        &mut client,
        &core_pub_key,
        &secp,
        &secret_key,
        timestamp,
        &public_key_vec,
    )
    .await;
    assert!(
        response.is_ok(),
        "Import user failed: {}",
        response.unwrap_err()
    );

    // Invalid signature: more than 10 minutes in the past
    let timestamp = (chrono::Utc::now() - Duration::minutes(11)).timestamp();
    let response = send_import_user_request(
        &mut client,
        &core_pub_key,
        &secp,
        &secret_key,
        timestamp,
        &public_key_vec,
    )
    .await;
    assert!(response.is_err(), "This signature should be invalid");

    // Invalid signature: more than 10 minutes in the future
    let timestamp = (chrono::Utc::now() + Duration::minutes(11)).timestamp();
    let response = send_import_user_request(
        &mut client,
        &core_pub_key,
        &secp,
        &secret_key,
        timestamp,
        &public_key_vec,
    )
    .await;
    assert!(response.is_err(), "This signature should be invalid");
}

async fn test_storage_crud() {
    // This is hardcoded because it requires a static string,
    // and there's no way to use format! to generate a static string unless using macros.
    let channel = Channel::from_static("http://127.0.0.1:10000")
        .connect()
        .await
        .unwrap();

    let mut client = CoLinkClient::new(channel);

    // Import New user
    // Get core pubkey first
    let mut core_info_request = tonic::Request::new(Empty {});
    core_info_request
        .metadata_mut()
        .insert("authorization", MetadataValue::from_static(""));
    let core_info_response = client.request_core_info(core_info_request).await.unwrap();
    let core_info: CoreInfo = core_info_response.into_inner();
    let core_pub_key = core_info.core_public_key;
    let core_pub_key = secp256k1::PublicKey::from_slice(&core_pub_key).unwrap();

    let secp = Secp256k1::new();
    let (secret_key, public_key) = secp.generate_keypair(&mut secp256k1::rand::thread_rng());
    let timestamp = chrono::Utc::now().timestamp();
    let public_key_vec = public_key.serialize().to_vec();
    let response = send_import_user_request(
        &mut client,
        &core_pub_key,
        &secp,
        &secret_key,
        timestamp,
        &public_key_vec,
    )
    .await
    .unwrap();
    let response: Jwt = response.into_inner();
    let jwt: String = response.jwt;

    let key_name_and_payload_1 = StorageEntry {
        key_name: "test_key_name".to_string(),
        key_path: Default::default(),
        payload: "test_payload".to_string().into_bytes(),
    };

    let key_name_and_payload_2 = StorageEntry {
        key_name: "test_key_name".to_string(),
        key_path: Default::default(),
        payload: "test_different_payload".to_string().into_bytes(),
    };

    let key_name = StorageEntry {
        key_name: "test_key_name".to_string(),
        key_path: Default::default(),
        payload: Default::default(),
    };

    let keys_to_read = StorageEntries {
        entries: vec![key_name.clone()],
    };

    // Create new entry
    let request = generate_request(&jwt, key_name_and_payload_1.clone()).await;
    let response = client.create_entry(request).await.unwrap();
    let response: StorageEntry = response.into_inner();
    println!(
        "Test: The first create entry response should be ok: {:?}",
        response
    );

    // Create same entry, should error
    let responded_key_path: String = response.key_path;
    let request = generate_request(&jwt, key_name_and_payload_1.clone()).await;
    let response = client.create_entry(request).await;
    assert!(
        response.is_err(),
        "Test: this response should fail, created same key name twice"
    );

    // Read entry
    let request = generate_request(&jwt, keys_to_read.clone()).await;
    let response = client.read_entries(request).await.unwrap();
    let response: StorageEntries = response.into_inner();
    let v: String = String::from_utf8(response.entries[0].payload.clone()).unwrap();
    println!("Test: read response should be ok: {:?}", v);

    // Update entry
    let request = generate_request(&jwt, key_name_and_payload_2.clone()).await;
    let response = client.update_entry(request).await.unwrap();
    let response: StorageEntry = response.into_inner();
    println!(
        "Test: response after update should return key path: {:?}",
        response
    );

    // Read entry after update
    let request = generate_request(&jwt, keys_to_read.clone()).await;
    let response = client.read_entries(request).await.unwrap();
    let response: StorageEntries = response.into_inner();
    let v: String = String::from_utf8(response.entries[0].payload.clone()).unwrap();
    println!("Test: read response after update should be ok: {:?}", v);

    let mut keys_to_read2 = keys_to_read.clone();
    keys_to_read2.entries.push(StorageEntry {
        key_name: Default::default(),
        key_path: responded_key_path.clone(),
        payload: Default::default(),
    });

    // Read entries with a key path and a key name
    let request = generate_request(&jwt, keys_to_read2.clone()).await;
    let response = client.read_entries(request).await.unwrap();
    let response: StorageEntries = response.into_inner();
    println!(
        "Test: read response should be now also contain old value: {:?}",
        response
    );

    // Delete entry
    let request = generate_request(&jwt, key_name.clone()).await;
    client.delete_entry(request).await.unwrap();
    let request = generate_request(&jwt, keys_to_read.clone()).await;
    let response = client.read_entries(request).await;
    assert!(
        response.is_err(),
        "Test: read response should be empty after deleted"
    );

    // Read keys, should contain deleted key(s) both with and without include_history
    let prefix = responded_key_path[0..responded_key_path.find(':').unwrap() + 1].to_string();
    println!("Test: prefix: {:?}", prefix);
    let request = generate_request(
        &jwt,
        ReadKeysRequest {
            include_history: false,
            prefix: prefix.clone(),
        },
    )
    .await;
    let response = client.read_keys(request).await.unwrap();
    let response: StorageEntries = response.into_inner();
    println!(
        "Test: We should see the timestamp of last update: {:?}",
        response
    );

    let request = generate_request(
        &jwt,
        ReadKeysRequest {
            include_history: true,
            prefix: prefix.clone(),
        },
    )
    .await;
    let response = client.read_keys(request).await.unwrap();
    let response: StorageEntries = response.into_inner();
    println!("Test: We should see the all timestamps: {:?}", response);
}
