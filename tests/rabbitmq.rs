use ::colink_core::mq::{common::MQ, rabbitmq::RabbitMQ};

const MQ_AMQP: &str = "amqp://guest:guest@localhost:5672";
const MQ_API: &str = "http://guest:guest@localhost:15672/api";
const MQ_PREFIX: &str = "colink-test";

#[tokio::test]
async fn user_and_vhost() -> Result<(), Box<dyn std::error::Error>> {
    let mq = RabbitMQ::new(MQ_AMQP, MQ_API, MQ_PREFIX);
    let uri = mq.create_user_account().await?;
    println!("MQ URI: {}", uri);
    let queue_name = mq.create_queue(&uri, "").await?;
    mq.delete_queue(&uri, &queue_name).await?;
    mq.delete_user_account(&uri).await?;
    mq.create_user_account().await?;
    mq.delete_all_accounts().await?;
    Ok(())
}
