use ::colink_server::mq::{common::MQ, rabbitmq::RabbitMQ, redis::RedisStream};

#[tokio::test]
async fn user_and_vhost() -> Result<(), Box<dyn std::error::Error>> {
    let mq: Box<dyn MQ> = if std::env::var("COLINK_TEST_MQ").is_ok()
        && std::env::var("COLINK_TEST_MQ").unwrap() == "redis"
    {
        Box::new(RedisStream::new("redis://localhost"))
    } else {
        Box::new(RabbitMQ::new(
            "amqp://guest:guest@localhost:5672",
            "http://guest:guest@localhost:15672/api",
            "colink-test",
        ))
    };
    let uri = mq.create_user_account().await?;
    println!("MQ URI: {}", uri);
    let queue_name = mq.create_queue(&uri, "").await?;
    mq.delete_queue(&uri, &queue_name).await?;
    mq.delete_user_account(&uri).await?;
    mq.create_user_account().await?;
    mq.delete_all_accounts().await?;
    Ok(())
}
