use ::colink_server::mq::{common::MQ, rabbitmq::RabbitMQ, redis::RedisStream};

#[rstest::rstest]
#[case(Box::new(RabbitMQ::new(
    "amqp://guest:guest@localhost:5672",
    "http://guest:guest@localhost:15672/api",
    "colink-test"
)))]
#[case(Box::new(RedisStream::new("redis://localhost")))]
#[tokio::test]
async fn user_and_vhost(#[case] mq: Box<dyn MQ>) -> Result<(), Box<dyn std::error::Error>> {
    let uri = mq.create_user_account().await?;
    println!("MQ URI: {}", uri);
    let queue_name = mq.create_queue(&uri, "").await?;
    mq.delete_queue(&uri, &queue_name).await?;
    mq.delete_user_account(&uri).await?;
    mq.create_user_account().await?;
    mq.delete_all_accounts().await?;
    Ok(())
}
