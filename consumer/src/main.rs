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
use serde::{Deserialize};
// use scylla::query::Query;
use scylla::retry_policy::{DefaultRetryPolicy};
use scylla::statement::{Consistency};
use scylla::transport::session::Session;
use scylla::transport::ExecutionProfile;
use scylla::frame::value::CqlTimestamp;
use scylla::{SessionBuilder};
use uuid::Uuid;
use std::env;

async fn get_db() -> String {
    match env::var("SCYLLA_URL") {
        Ok(s) => s,
        _ => "127.0.0.1:9042".to_string()
    }
}

async fn get_topic() -> String {
    match env::var("REDPANDA_URL") {
        Ok(s) => s,
        _ => "127.0.0.1:19092".to_string()
    }
}

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

#[derive(Deserialize)]
struct Post {
    id: Uuid,
    subject: String,
    content: String,
}

#[derive(Deserialize)]
struct WinnerData {
    id: Uuid,
    email: String,
}

#[derive(Deserialize)]
struct PostLike {
    post_id: Uuid,
}

#[derive(Deserialize)]
struct Follow {
    id: Uuid,
    followed_id: Uuid,
}

#[derive(Deserialize)]
struct QueryId {
    id: Uuid,
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
        let save_post = session.prepare("INSERT INTO ks.posts (id, ts, post_id, subject, content) VALUES (?, ?, ?, ?, ? )").await.unwrap();
        let profile_views = session.prepare("UPDATE ks.profile_views SET count = count + 1 WHERE id = ?").await.unwrap();
        let add_follower = session.prepare("INSERT INTO ks.follows (id, follower, ts) VALUES (?, ?, ?)").await.unwrap();
        let post_like = session.prepare("UPDATE ks.post_likes SET count = count + 1 WHERE post_id = ?").await.unwrap();
        let game_draw = session.prepare("UPDATE ks.game SET ts = ?, winner = ? WHERE id = ?").await.unwrap();
        let winner_data = session.prepare("UPDATE ks.game SET email = ? WHERE id = ?").await.unwrap();

        let null: Option<String> = None;
        match evtype {
            "CREATE" => {
                let object: User = serde_json::from_str(r).unwrap();
                session.execute(&add_user, (object.id, object.username, CqlTimestamp(ts.timestamp() * 1000))).await.unwrap();
                session.execute(&store_event, (object.id, CqlTimestamp(ts.timestamp() * 1000), evtype, null)).await.unwrap();
            },
            "PAGE_VIEW" => {
                let object: PageView = serde_json::from_str(r).unwrap();
                session.execute(&page_count, (&object.page,)).await.unwrap();
                session.execute(&store_event, (object.id, CqlTimestamp(ts.timestamp() * 1000), evtype, object.page)).await.unwrap();
            }, 
            "POST" => {
                let object: Post = serde_json::from_str(r).unwrap();
                let post_id = Uuid::new_v4();
                session.execute(&save_post, (object.id, CqlTimestamp(ts.timestamp() * 1000), post_id, object.subject, object.content)).await.unwrap();
                session.execute(&store_event, (object.id, CqlTimestamp(ts.timestamp() * 1000), evtype, null)).await.unwrap();
            },
            "POST_LIKE" => {
                let object: PostLike = serde_json::from_str(r).unwrap();
                session.execute(&post_like, (object.post_id,)).await.unwrap();
                // Note: post_id is used here, find engagement on Posts of a specific user.
                session.execute(&store_event, (object.post_id, CqlTimestamp(ts.timestamp() * 1000), evtype, null)).await.unwrap();
            },
            "FOLLOW" => {
                let object: Follow = serde_json::from_str(r).unwrap();
                session.execute(&add_follower, (object.id, object.followed_id, CqlTimestamp(ts.timestamp() * 1000))).await.unwrap();
                session.execute(&store_event, (object.id, CqlTimestamp(ts.timestamp() * 1000), evtype, null)).await.unwrap();
            },
            "USER_VIEW" => {
                let object: QueryId = serde_json::from_str(r).unwrap();
                // No need to generate an event, as we already know before-hand from PAGE_VIEW.
                session.execute(&profile_views, (object.id,)).await.unwrap();
            },
            "GAME_DRAW" => {
                let object: QueryId = serde_json::from_str(r).unwrap();
                session.execute(&game_draw, (CqlTimestamp(ts.timestamp() * 1000), false, object.id,)).await.unwrap();
                session.execute(&store_event, (object.id, CqlTimestamp(ts.timestamp() * 1000), evtype, null)).await.unwrap();
            },
            "WINNER_SUBMIT" => {
                let object: WinnerData = serde_json::from_str(r).unwrap();
                session.execute(&winner_data, (object.email, object.id,)).await.unwrap();
                println!("User {} Won!", object.id);
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
        .known_node(get_db().await)
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
    session.query("CREATE TABLE IF NOT EXISTS ks.follows (id uuid, follower uuid, ts timestamp, PRIMARY KEY(id, follower))",
                  &[],
                  ).await.unwrap();
    session.query("CREATE MATERIALIZED VIEW IF NOT EXISTS ks.followers AS SELECT * FROM ks.follows WHERE id IS NOT NULL AND follower IS NOT NULL PRIMARY KEY(follower, id)",
                  &[],
                  ).await.unwrap();
    session.query("CREATE TABLE IF NOT EXISTS ks.posts (id uuid, post_id uuid, ts timestamp, subject text, content text, PRIMARY KEY(id, ts, post_id)) WITH CLUSTERING ORDER BY (ts DESC)",
                  &[],
                  ).await.unwrap();
    session.query("CREATE TABLE IF NOT EXISTS ks.post_likes (post_id uuid PRIMARY KEY, count counter)", &[]).await.unwrap();
    session.query("CREATE TABLE IF NOT EXISTS ks.game (id uuid PRIMARY KEY, ts timestamp, winner boolean, email text)", &[]).await.unwrap();
    session.query("CREATE TABLE IF NOT EXISTS ks.winner (id uuid PRIMARY KEY)", &[]).await.unwrap();

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

    let client: Arc<Client> = broker_client(get_topic().await).await;
    let viewclient: Arc<Client> = broker_client(get_topic().await).await;
    let postclient: Arc<Client> = broker_client(get_topic().await).await;
    let followclient: Arc<Client> = broker_client(get_topic().await).await;
    let gameclient: Arc<Client> = broker_client(get_topic().await).await;

	ensure_topic_exists(&client, "identity").await;
	ensure_topic_exists(&client, "pageview").await;
	ensure_topic_exists(&client, "posts").await;
	ensure_topic_exists(&client, "game").await;

    tokio::spawn(async move {
        use std::time::Duration;
        println!("Identity Consumer Started");
        let mut offset = 0;
        let session = db_session().await;
        loop {
            offset = consumer(&client, "identity", offset, &session).await;
            thread::sleep(Duration::from_secs(5));
            //println!("[Identity] - Sleeping")
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
        }
    });

    tokio::spawn(async move {
        use std::time::Duration;
        println!("Posts Consumer Started");
        let mut offset = 0;
        let session = db_session().await;
        loop {
            offset = consumer(&postclient, "posts", offset, &session).await;
            thread::sleep(Duration::from_secs(5));
        }
    });

    tokio::spawn(async move {
        use std::time::Duration;
        println!("Follower Consumer Started");
        let mut offset = 0;
        let session = db_session().await;
        loop {
            offset = consumer(&followclient, "follow", offset, &session).await;
            thread::sleep(Duration::from_secs(5));
        }
    });

    tokio::spawn(async move {
        use std::time::Duration;
        println!("Game Consumer Started");
        let mut offset = 0;
        let session = db_session().await;
        loop {
            offset = consumer(&gameclient, "game", offset, &session).await;
            thread::sleep(Duration::from_secs(5));
        }
    });

    // Just sleep
    loop {
        sleep(Duration::from_secs(86400)).await;
    }
}
