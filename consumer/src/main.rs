use tokio;
use tokio::time::sleep;
use std::str;
use std::time::Duration;
use std::thread;
use std::sync::Arc;
use rskafka::{
    client::{
        ClientBuilder,
        partition::{UnknownTopicHandling},
    },
    record::RecordAndOffset,
};
use rskafka::client::Client;
use scylla::load_balancing;
use serde_json::Value;
use serde::{Deserialize};
// use scylla::query::Query;
use scylla::retry_policy::{DefaultRetryPolicy};
use scylla::statement::{Consistency};
use scylla::transport::session::Session;
use scylla::transport::ExecutionProfile;
use scylla::frame::value::CqlTimestamp;
use scylla::{SessionBuilder};
use uuid::Uuid;

async fn ensure_topic_exists(client: &Client, topic_name: &str) {
    let controller_client = client.controller_client().unwrap();

    let topic_list = client.list_topics().await.unwrap();
    let mut create = true;
    for topic in topic_list.iter() {
        if topic.name == topic_name {
            create = false;
            break;
        }   
        else {
            create = true;
        }   
    }   

    if create {
       controller_client.create_topic(
          topic_name,
          2,      // partitions
          1,      // replication factor
          5_000,  // timeout (ms)
        ).await.unwrap();
    }   

}

#[derive(Deserialize)]
struct User {
    id: Uuid,
    username: String,
}

#[derive(Deserialize)]
struct PageView {
    id: Uuid,
    page: String,
}

async fn process_records(record: Vec<RecordAndOffset>, session: &Session) {
    // note: available fields are: `key`, `value`, `headers`, `timestamp`
    for i in record.iter() {
        let ts = i.record.timestamp;

        let s = match &i.record.value {
            Some(x) => x,
            None => continue,
        };

        let r = match str::from_utf8(s) {
            Ok(v) => v,
            Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
        };

        let h = &i.record.headers.get("EVENT_TYPE");
        let v = match &h {
            Some(x) => x,
            None => continue,
        };

        let evtype = match str::from_utf8(v) {
            Ok(v) => v,
            Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
        };

        println!("Processing {} event - {} - {} ", evtype, ts, r);

        let add_user = session.prepare("INSERT INTO ks.users (id, name, registration_date) VALUES (?, ?, ?)").await.unwrap();
        let store_event = session.prepare("INSERT INTO ks.events (id, ts, event_type, src_page) VALUES (?, ?, ?, ?)").await.unwrap();
        let page_count = session.prepare("UPDATE ks.page_count SET count = count + 1 WHERE page = ?").await.unwrap();
        // Remove comments once the time is right. Avoid Rust compiler from screaming
        //let profile_views = session.prepare("UPDATE ks.profile_views SET count = count + 1 WHERE id = ?");
        //let add_follower = session.prepare("INSERT INTO ks.followers (id, follower, ts) VALUES (?, ?, ?)");

        match evtype {
            "CREATE" => {
                let object: User = serde_json::from_str(r).unwrap();
                let null: Option<String> = None;
                session.execute(&add_user, (object.id, object.username, CqlTimestamp(ts.timestamp() * 1000))).await.unwrap();
                session.execute(&store_event, (object.id, CqlTimestamp(ts.timestamp() * 1000), evtype, null)).await.unwrap();
            },
            "PAGE_VIEW" => {
                let object: PageView = serde_json::from_str(r).unwrap();
                session.execute(&page_count, (&object.page,)).await.unwrap();
                session.execute(&store_event, (object.id, CqlTimestamp(ts.timestamp() * 1000), evtype, object.page)).await.unwrap();
            }, 
            _ => println!("Unknown event {}", evtype),
        }

    }
}

async fn db_session() -> Session {
    let profile = ExecutionProfile::builder()
        .consistency(Consistency::LocalQuorum)
        .request_timeout(Some(Duration::from_secs(10)))
        .load_balancing_policy(Arc::new(load_balancing::DefaultPolicy::default()))
        .retry_policy(Box::new(DefaultRetryPolicy::new()))
        .speculative_execution_policy(None)
        .build();

    let handle = profile.clone().into_handle();

    let session: Session = SessionBuilder::new()
        .known_node("127.0.0.1:9042")
        .default_execution_profile_handle(handle.clone())
        .build()
        .await.unwrap();

    // Best to prep the environment so we don't call CREATE often later on
    session.query("CREATE KEYSPACE IF NOT EXISTS ks WITH REPLICATION = { 'class': 'NetworkTopologyStrategy', 'replication_factor': 1}", &[]).await.unwrap();
    session.query("CREATE TABLE IF NOT EXISTS ks.events (id uuid, ts timestamp, event_type text, src_page text, PRIMARY KEY(id, event_type, ts))",
                  &[],
                  ).await.unwrap();
    session.query("CREATE TABLE IF NOT EXISTS ks.users (id uuid PRIMARY KEY, name text, registration_date timestamp)", &[]).await.unwrap();
    session.query("CREATE TABLE IF NOT EXISTS ks.page_count (page text PRIMARY KEY, count counter)", &[]).await.unwrap();
    session.query("CREATE TABLE IF NOT EXISTS ks.profile_views (id uuid PRIMARY KEY, count counter)", &[]).await.unwrap();
    session.query("CREATE TABLE IF NOT EXISTS ks.followers (id uuid, follower uuid, ts timestamp, PRIMARY KEY(id, follower))",
                  &[],
                  ).await.unwrap();

    session
}

async fn broker_client(connection: String) -> Arc<Client> {
    let con: Arc<Client> = Arc::new(ClientBuilder::new(vec![connection]).build().await.unwrap());
    con
}

async fn consumer(client: &Client, topic: &str, offset: i64, session: &Session) -> i64 {

    let partition_client=Arc::new(client.partition_client(
            topic,
            0,  
            UnknownTopicHandling::Retry,
            ).await.unwrap()
        );  

    let (record, end) = partition_client
        .fetch_records(
            offset, // offset
            1..1_000_000_000,   // min..max bytes
            1_000,   // max wait time
        )   
        .await
        .unwrap();

    process_records(record, session).await;

    end
}

#[tokio::main]
async fn main() {

    let client: Arc<Client> = broker_client("localhost:19092".to_string()).await;
    let viewclient: Arc<Client> = broker_client("localhost:19092".to_string()).await;
	ensure_topic_exists(&client, "identity").await;
	ensure_topic_exists(&client, "pageview").await;

    tokio::spawn(async move {
        use std::time::Duration;
        println!("Identity Consumer Started");
        let mut offset = 0;
        let session = db_session().await;
        loop {
            offset = consumer(&client, "identity", offset, &session).await;
            thread::sleep(Duration::from_secs(5));
            println!("[Identity] - Sleeping")
        }
    });

    tokio::spawn(async move {
        use std::time::Duration;
        println!("PageView Consumer Started");
        let mut offset = 0;
        let session = db_session().await;
        loop {
            offset = consumer(&viewclient, "pageview", offset, &session).await;
            thread::sleep(Duration::from_secs(5));
            println!("[PageView] - Sleeping")
        }
    });

    // Just sleep
    loop {
        sleep(Duration::from_secs(86400)).await;
    }
}
