use tokio;
use std::sync::Arc;
use std::collections::BTreeMap;
use chrono::Utc;
use axum::{
    routing::{get, post},
    http::StatusCode,
    Json, Router,
};
use axum::extract::State;
use serde::{Deserialize, Serialize};
use fake::{Fake};
use fake::faker::company::en::*;
use uuid::Uuid;
use rskafka::{
    client::{
        ClientBuilder, Client,
        partition::{Compression, UnknownTopicHandling},
    },
    record::Record,
};

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

async fn produce_message(client: &Client, topic_name: &str, record: Record) {
    // get a partition-bound client
    let partition_client = client
        .partition_client(
            topic_name.to_owned(),
            0,  // partition
            UnknownTopicHandling::Retry,
         )   
         .await
        .unwrap();

	partition_client.produce(vec![record], Compression::default()).await.unwrap();
}

async fn broker_client(connection: String) -> Arc<Client> {
	let con: Arc<Client> = Arc::new(ClientBuilder::new(vec![connection]).build().await.unwrap());
    con
}

#[tokio::main]
async fn main() {
	let client: Arc<Client> = broker_client("localhost:19092".to_string()).await;
    let views: Arc<Client> = broker_client("localhost:19092".to_string()).await;
	ensure_topic_exists(&client, "identity").await;
	ensure_topic_exists(&client, "pageview").await;

	// TODO: Custom Pool impl? It is easy to saturate a few connections.
    let app = Router::new()
        // `GET /` goes to `root`
        .route("/", get(root))
        // `POST /users` goes to `create_user`
        .route("/users", get(create_user))
		.route("/view", post(view))
        .with_state(client)
        .with_state(views);

    // run our app with hyper, listening globally on port 3001
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// basic handler that responds with a static string
async fn root() -> &'static str {
    "Hello, World!"
}

async fn create_user(State(client): State<Arc<Client>>, State(views): State<Arc<Client>>) -> (StatusCode, Json<User>) {
    // insert your application logic here
    let id: String = Uuid::new_v4().to_string();
    let buzz: String = Buzzword().fake();
    let profession: String = Profession().fake();
    let username = format!("{} {}", buzz, profession);
    let payload = format!(r#"{{ "id": "{id}", "username": "{username}" }}"#).into_bytes().to_vec();

    let user = User {
        id: id.clone(),
        //username: payload.username,
        username: username,
    };

    // produce some data
    let record = Record {
        key: None,
        value: Some(payload),
        headers: BTreeMap::from([
           ("EVENT_TYPE".to_owned(), b"CREATE".to_vec()),
        ]),
        timestamp: Utc::now(),
    };

	produce_message(&client, "identity", record).await;

	let page_browsed = PageBrowsed {
		id: id.clone(),
		page: "/".to_string(),
	};

	view(State(views), Json(page_browsed)).await;

    // this will be converted into a JSON response
    // with a status code of `201 Created`
    (StatusCode::CREATED, Json(user))
}

async fn view( State(views): State<Arc<Client>>, Json(payload): Json<PageBrowsed>, ) -> StatusCode {
	println!("{} {}", payload.id, payload.page);	

	let payload = format!(r#"{{ "id": "{0}", "page": "{1}" }}"#, payload.id, payload.page).into_bytes().to_vec();
	
	let record = Record {
		key: None,
		value: Some(payload),
		headers: BTreeMap::from([
			("EVENT_TYPE".to_owned(), b"PAGE_VIEW".to_vec()),
		]),
		timestamp: Utc::now(),
	};

	produce_message(&views, "pageview", record).await;

	StatusCode::CREATED
}

// the input to our `pageBrowsed` handler
#[derive(Deserialize)]
struct PageBrowsed {
    id: String,
	page: String,
}

// the output to our `create_user` handler
#[derive(Serialize)]
struct User {
    id: String,
    username: String,
}
