use tokio;
use std::sync::Arc;
use std::collections::BTreeMap;
use chrono::Utc;
use axum::{
    routing::{get, post},
    http::StatusCode,
    Json, Router,
};
use http::Method;
use http::header::CONTENT_TYPE;
use tower_http::cors::{Any, CorsLayer};
use axum::extract::State;
use axum::extract::Path;
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
use scylla::load_balancing;
use scylla::retry_policy::{DefaultRetryPolicy};
use scylla::statement::{Consistency};
use scylla::transport::session::Session;
use scylla::transport::ExecutionProfile;
use scylla::macros::SerializeRow;
use scylla::frame::value::CqlTimestamp;
use scylla::frame::value::Counter;
use scylla::{SessionBuilder};
use std::time::Duration;
use rand::seq::SliceRandom;
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

async fn db_session() -> Arc<Session> {
    let profile = ExecutionProfile::builder()
        .consistency(Consistency::LocalQuorum)
        .request_timeout(Some(Duration::from_secs(10)))
        .load_balancing_policy(Arc::new(load_balancing::DefaultPolicy::default()))
        .retry_policy(Box::new(DefaultRetryPolicy::new()))
        .speculative_execution_policy(None)
        .build();

    let handle = profile.clone().into_handle();

    let session: Arc<Session> = SessionBuilder::new()
        .known_node(&get_db().await)
        .default_execution_profile_handle(handle.clone())
        .build()
        .await.unwrap().into();

    session
}


#[tokio::main]
async fn main() {
	let client: Arc<Client> = broker_client(get_topic().await).await;
    let views: Arc<Client> = broker_client(get_topic().await).await;
    let session: Arc<Session> = db_session().await;

	ensure_topic_exists(&client, "identity").await;
	ensure_topic_exists(&client, "pageview").await;
    ensure_topic_exists(&client, "posts").await;
    ensure_topic_exists(&client, "follow").await;

    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_origin(Any)
        .allow_headers([CONTENT_TYPE]);

    let app = Router::new()
        // `GET /` goes to `root`
        .route("/", get(root))
        .route("/users", get(create_user))
		.route("/view", post(view))
        .route("/new_post", post(new_post)) 
        .route("/draw/submit", post(winner_submit)) 
        .route("/follow", post(follow))
        .route("/like_post/:post_id", get(like_post))
        .route("/user/:id/count", get(user_increment))
        .route("/draw/:id", get(game_drawing))
        .with_state(client)
        .with_state(views)
        .route("/query", post(query_users))
        .route("/list", get(userlist))
        .route("/user/:id", get(user))
        .route("/posts/:id", get(posts_by_user))
        .route("/post/:post_id/likes", get(post_likes))
        .route("/followers/:id", get(followers_by_id))
        .route("/followed/:id", get(followed_by_id))
        .route("/page_hits", get(page_hits))
        .route("/profile_hits", get(profile_hits))
        .route("/draw_winner", get(draw_winner))
        .route("/reset_winner", get(reset_winner))
        .route("/draw/status", get(draw_status))
        .route("/draw/result", get(draw_result))
        .with_state(session)
        .layer(cors);

    // run our app with hyper, listening globally on port 3001
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// basic handler that responds with a static string
async fn root() -> &'static str {
    "Hello, World!"
}

async fn userlist(State(session):State<Arc<Session>>) -> (StatusCode, Json<Vec<UserInfo>>) {
    let result = session.query("SELECT id, name, registration_date FROM ks.users", &[]).await.unwrap();
    let rows = result.rows.unwrap();

    let mut x: Vec<UserInfo> = vec![];
    for row in rows {
        let (id, name, registration_date) = row.into_typed::<(Uuid, String, CqlTimestamp)>().unwrap();
        let y = UserInfo {
            id: id,
            name: name,
            regdate: registration_date.0,
        };
        x.push(y);
    }

    (StatusCode::OK, Json(x))

}

async fn page_hits(State(session): State<Arc<Session>>) -> (StatusCode, Json<Vec<Pages>>){
    let result = session.query("SELECT page, count FROM ks.page_count", &[]).await.unwrap();
    let rows = result.rows.unwrap();

    let mut x: Vec<Pages> = vec![];
    for row in rows {
        let (page, count) = row.into_typed::<(String, Counter)>().unwrap();
        let y = Pages {
            page: page,
            count: count.0,
        };
        x.push(y);
    }

    (StatusCode::OK, Json(x))

}

async fn draw_winner(State(session): State<Arc<Session>>) -> StatusCode {
    let ps = session.prepare("UPDATE ks.game SET winner = true WHERE id = ?").await.unwrap();
    let contestants = session.query("SELECT id FROM ks.game", &[]).await.unwrap();
    let rows = contestants.rows.unwrap();

    let mut x: Vec<QueryId> = vec![];

    for row in rows {
        let id = row.columns[0].as_ref().unwrap().as_uuid().unwrap();
        let y = QueryId {
            id: id,
        };
        x.push(y);
    }

    let winner: Vec<_> = x
        .choose_multiple(&mut rand::thread_rng(), 1)
        .collect();

    let _w = session.execute(&ps, winner[0]).await.unwrap();
    
    // We've got a winner, allow individuals to check whether they won
    let ps = session.prepare("INSERT INTO ks.winner (id) VALUES (?)").await.unwrap();
    let _result = session.execute(&ps, winner[0]).await.unwrap();

    StatusCode::OK
}

async fn draw_status(State(session): State<Arc<Session>>) -> (StatusCode, Json<GameStatus>) {
    let result = session.query("SELECT COUNT(1) FROM ks.winner", &[]).await.unwrap();
    let rows = result.rows.unwrap();
    let x = rows[0].columns[0].as_ref().unwrap().as_bigint().unwrap();

    let response = match x {
        0 => GameStatus { status: false },
        _ => GameStatus { status: true },
    };

    (StatusCode::OK, Json(response))
}

async fn draw_result(State(session): State<Arc<Session>>) -> (StatusCode, Json<QueryId>) {
    let result = session.query("SELECT id FROM ks.winner LIMIT 1", &[]).await.unwrap();
    let rows = result.rows.unwrap();

    let id = rows[0].columns[0].as_ref().unwrap().as_uuid().unwrap();
    let response = QueryId {
        id: id,
    };

    (StatusCode::OK, Json(response))
}

async fn reset_winner(State(session): State<Arc<Session>>) -> StatusCode {
    let _ = session.query("TRUNCATE ks.winner", &[]).await.unwrap();
    let ps = session.prepare("UPDATE ks.game SET winner = false WHERE id = ?").await.unwrap();
    let contestants = session.query("SELECT id FROM ks.game", &[]).await.unwrap();
    let rows = contestants.rows.unwrap();

    for row in rows {
        let id = row.columns[0].as_ref().unwrap().as_uuid().unwrap();
        let y = QueryId {
            id: id,
        };
        let _ = session.execute(&ps, y).await.unwrap();
    }

    StatusCode::OK
}

async fn profile_hits(State(session): State<Arc<Session>>) -> (StatusCode, Json<Vec<ProfileHits>>) {
    let profile_stmt = session.prepare("SELECT name FROM ks.users WHERE id = ?").await.unwrap();
    let result = session.query("SELECT id, count FROM ks.profile_views", &[]).await.unwrap();
    let rows = result.rows.unwrap();

    let mut x: Vec<ProfileHits> = vec![];
    for row in rows {
        let (profile_id, count) = row.into_typed::<(Uuid, Counter)>().unwrap();

        let id = QueryId {
            id: profile_id.to_owned(),
        };

        let profile_res = session.execute(&profile_stmt, &id).await.unwrap();
        let profile_row = profile_res.rows.unwrap();
        let profile_name = profile_row[0].columns[0].as_ref().unwrap().as_text().unwrap();

        let y = ProfileHits {
            profile_id: profile_id,
            profile_name: profile_name.to_string(),
            count: count.0,
        };
        x.push(y);

    }

    (StatusCode::OK, Json(x))

}
// In this method we retrieve all followers and then retrieve their data from the main table.
// Ideally, the needed contents should be in a single table to satisfy the query and avoid these
// round-trips.
async fn followers_by_id(State(session): State<Arc<Session>>, Path(id): Path<Uuid>, ) -> (StatusCode, Json<Vec<UserInfo>>) {
    let followers_ps = session.prepare("SELECT id, ts FROM ks.followers WHERE follower = ?").await.unwrap();
    let follow_id = FollowerId {
        follower: id.to_owned(),
    };

    let result = session.execute(&followers_ps, &follow_id).await.unwrap();
    let rows = result.rows.unwrap();
    let mut x: Vec<UserInfo> = vec![];
    let mut z: Vec<FollowerInfo> = vec![];
    for row in rows {
        let (id, ts) = row.into_typed::<(Uuid, CqlTimestamp)>().unwrap();
        let y = FollowerInfo {
            id: id,
            ts: ts.0,
        };
        z.push(y);
    }

    // At this point we have FollowerInfo with all followers.
    // Retrieve each ones data. TODO: Why not call the existing endpoint lol?
    let ps = session.prepare("SELECT name FROM ks.users WHERE id = ?").await.unwrap();
    for profile in z {
        let id = QueryId {
            id: profile.id,
        };
        let result = session.execute(&ps, &id).await.unwrap();
        let rows = result.rows.unwrap();
        let follower_name = rows[0].columns[0].as_ref().unwrap().as_text().unwrap();

        // Ok, we got the missing field. Build the final struct.
        let y = UserInfo {
            id: profile.id,
            name: follower_name.to_string(),
            regdate: profile.ts
        };

        x.push(y);
    }

    (StatusCode::OK, Json(x))
}

async fn followed_by_id(State(session): State<Arc<Session>>, Path(id): Path<Uuid>, ) -> (StatusCode, Json<Vec<UserInfo>>) {
    let followers_ps = session.prepare("SELECT follower, ts FROM ks.follows WHERE id = ?").await.unwrap();
    let id = QueryId {
        id: id.to_owned(),
    };

    let result = session.execute(&followers_ps, &id).await.unwrap();
    let rows = result.rows.unwrap();
    let mut x: Vec<UserInfo> = vec![];
    let mut z: Vec<FollowerInfo> = vec![];
    for row in rows {
        let (id, ts) = row.into_typed::<(Uuid, CqlTimestamp)>().unwrap();
        let y = FollowerInfo {
            id: id,
            ts: ts.0,
        };
        z.push(y);
    }

    // At this point we have FollowerInfo with all followers.
    // Retrieve each ones data. TODO: Why not call the existing endpoint lol?
    let ps = session.prepare("SELECT name FROM ks.users WHERE id = ?").await.unwrap();
    for profile in z {
        let id = QueryId {
            id: profile.id,
        };
        let result = session.execute(&ps, &id).await.unwrap();
        let rows = result.rows.unwrap();
        let follower_name = rows[0].columns[0].as_ref().unwrap().as_text().unwrap();

        // Ok, we got the missing field. Build the final struct.
        let y = UserInfo {
            id: profile.id,
            name: follower_name.to_string(),
            regdate: profile.ts
        };

        x.push(y);
    }

    (StatusCode::OK, Json(x))
}


async fn post_likes(State(session): State<Arc<Session>>, Path(post_id): Path<Uuid>, ) -> (StatusCode, Json<PostLike>) {
    let ps = session.prepare("SELECT count FROM ks.post_likes WHERE post_id = ?").await.unwrap();
    let post_id = PostId {
        post_id: post_id,
    };

    let result = session.execute(&ps, &post_id).await.unwrap();
    let rows = result.rows.unwrap();
    let mut like_count: i64 = 0;
    if rows.len() > 0 {
        like_count = rows[0].columns[0].as_ref().unwrap().as_counter().unwrap().0;
    }
    let res = PostLike {
        count: like_count,
    };

    (StatusCode::OK, Json(res))
}

async fn user(State(session): State<Arc<Session>>, Path(id): Path<Uuid>, ) -> (StatusCode, Json<UserInfo>) {
    let ps = session.prepare("SELECT id, name, registration_date FROM ks.users WHERE id = ?").await.unwrap();
    let id = QueryId {
        id: id,
    };
    let result = session.execute(&ps, &id).await.unwrap();
    let rows = result.rows.unwrap();

    if rows.len() == 0 {
        let y = UserInfo {
           id: id.id,
           name: "Dummy".to_string(),
           regdate: 0,
        };
        (StatusCode::NOT_FOUND, Json(y))
    } else {
        let mut y = UserInfo { id: id.id, name: "".to_string(), regdate: 0, };
        for row in rows {
          let (id, name, registration_date) = row.into_typed::<(Uuid, String, CqlTimestamp)>().unwrap();
            y = UserInfo {
              id: id,
              name: name,
              regdate: registration_date.0,
            };
        }
    (StatusCode::OK, Json(y))
    }

}

async fn posts_by_user(State(session): State<Arc<Session>>, Path(id): Path<Uuid>, ) -> (StatusCode, Json<Vec<PostResponse>>){
    let ps = session.prepare("SELECT ts, content, post_id, subject FROM ks.posts WHERE id = ? LIMIT 9").await.unwrap();
    let id = QueryId {
        id: id,
    };
    let result = session.execute(&ps, &id).await.unwrap();
    let rows = result.rows.unwrap();

    let mut x: Vec<PostResponse> = vec![];
    for row in rows {
        let (ts, content, post_id, subject) = row.into_typed::<(CqlTimestamp, String, Uuid, String)>().unwrap();
        let y = PostResponse {
            post_id: post_id.to_string(),
            subject: subject,
            content: content,
            ts: ts.0,
        };
        x.push(y);
    }

    (StatusCode::OK, Json(x))

}

async fn query_users(State(session): State<Arc<Session>>, Json(payload): Json<UserRequest>, ) -> (StatusCode, Json<UserResponse>) {

    let res = match payload.kind.as_str() {
        "count" => {
            let ps = session.prepare("SELECT COUNT(1) FROM ks.followers WHERE follower = ?").await.unwrap();
            let pview = session.prepare("SELECT count FROM ks.profile_views WHERE id = ?").await.unwrap();
            let follow_id = FollowerId {
                follower: payload.id.to_owned(),
            };
            let id = QueryId {
                id: payload.id.to_owned(),
            };

            let result = session.execute(&ps, &follow_id).await.unwrap();

            let rows = result.rows.unwrap();
            let followers = rows[0].columns[0].as_ref().unwrap().as_bigint().unwrap();

            let result = session.execute(&pview, id).await.unwrap();
            let rows = result.rows.unwrap();
            let pviews: i64 = match rows.len() {
                0 => 0,
                _ => {
                    let x: i64 = rows[0].columns[0].as_ref().unwrap().as_counter().unwrap().0;
                    x
                },
            };

            let response = UserResponse {
                id: payload.id,
                follower_count: followers,
                profile_views: pviews,
            };
            response
        },
        _ => {
            let response = UserResponse {
                id: payload.id,
                follower_count: 0,
                profile_views: 0,
            };
            response
        },
    };

    //let response = UserResponse {
    //    id: "someone".to_string(),
    //    follower_count: 0,
    //    whatever: None,
    //};

    (StatusCode::OK, Json(res))
}

async fn game_drawing(State(client): State<Arc<Client>>, Path(id): Path<Uuid>, ) -> StatusCode {
    let payload = format!(r#"{{ "id": "{0}" }}"#, id).into_bytes().to_vec();
    let record = Record {
        key: None,
        value: Some(payload),
        headers: BTreeMap::from([
           ("EVENT_TYPE".to_owned(), b"GAME_DRAW".to_vec()),
        ]),
        timestamp: Utc::now(),
    };

    produce_message(&client, "game", record).await;

    StatusCode::OK
}

async fn user_increment(State(client): State<Arc<Client>>, Path(user_id): Path<Uuid>, ) -> StatusCode {
    let payload = format!(r#"{{ "id": "{0}" }}"#, user_id).into_bytes().to_vec();
    let record = Record {
        key: None,
        value: Some(payload),
        headers: BTreeMap::from([
           ("EVENT_TYPE".to_owned(), b"USER_VIEW".to_vec()),
        ]),
        timestamp: Utc::now(),
    };

    produce_message(&client, "identity", record).await;

    StatusCode::OK
}

async fn like_post(State(client): State<Arc<Client>>, Path(post_id): Path<Uuid>, ) -> StatusCode {

    let payload = format!(r#"{{ "post_id": "{0}" }}"#, post_id.to_string()).into_bytes().to_vec();
    let record = Record {
        key: None,
        value: Some(payload),
        headers: BTreeMap::from([
           ("EVENT_TYPE".to_owned(), b"POST_LIKE".to_vec()),
        ]),
        timestamp: Utc::now(),
    };

    produce_message(&client, "posts", record).await;

    StatusCode::OK
}

async fn follow(State(client): State<Arc<Client>>, Json(payload): Json<Follow>, ) -> StatusCode {
    let payload = format!(r#"{{ "id": "{0}", "followed_id": "{1}" }}"#, payload.id, payload.followed_id).into_bytes().to_vec();

    let record = Record {
        key: None,
        value: Some(payload),
        headers: BTreeMap::from([
           ("EVENT_TYPE".to_owned(), b"FOLLOW".to_vec()),
        ]),
        timestamp: Utc::now(),
    };

    produce_message(&client, "follow", record).await;

    StatusCode::CREATED
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

async fn winner_submit(State(client): State<Arc<Client>>, Json(payload): Json<WinnerData>,) -> StatusCode {
    let payload = format!(r#"{{ "id": "{0}", "email": "{1}" }}"#, payload.id, payload.email)
        .into_bytes()
        .to_vec();

    let record = Record {
        key: None,
        value: Some(payload),
        headers: BTreeMap::from([
            ("EVENT_TYPE".to_owned(), b"WINNER_SUBMIT".to_vec()),
        ]),
        timestamp: Utc::now(),
    };

    produce_message(&client, "game", record).await;

    StatusCode::CREATED

}

async fn new_post(State(client): State<Arc<Client>>, Json(payload): Json<PostData>,) -> StatusCode {
    let payload = format!(r#"{{ "id": "{0}", "subject": "{1}", "content": "{2}" }}"#, payload.id, payload.subject, payload.content)
        .into_bytes()
        .to_vec();

    let record = Record {
        key: None,
        value: Some(payload),
        headers: BTreeMap::from([
            ("EVENT_TYPE".to_owned(), b"POST".to_vec()),
        ]),
        timestamp: Utc::now(),
    };

    produce_message(&client, "posts", record).await;

    StatusCode::CREATED
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

#[derive(Deserialize)]
struct WinnerData {
    id: String,
    email: String,
}

#[derive(Deserialize)]
struct PostData {
    id: String,
    subject: String,
    content: String,
}

// the output to our `create_user` handler
#[derive(Serialize)]
struct User {
    id: String,
    username: String,
}

#[derive(Serialize)]
struct UserResponse {
    id: Uuid,
    follower_count: i64,
    profile_views: i64,
}

#[derive(Deserialize)]
struct UserRequest {
    kind: String,
    id: Uuid,
}

#[derive(Serialize)]
struct PostResponse {
    post_id: String,
    subject: String,
    content: String,
    ts: i64,
}

#[derive(Serialize)]
struct GameStatus {
    status: bool,
}

#[derive(Serialize)]
struct PostLike {
    count: i64,
}

#[derive(Deserialize)]
struct Follow {
    id: String,
    followed_id: String,
}

#[derive(Debug, Serialize, SerializeRow)]
struct QueryId {
    id: Uuid,
}

#[derive(SerializeRow)]
struct PostId {
    post_id: Uuid,
}

#[derive(SerializeRow)]
struct FollowerId {
    follower: Uuid,
}

#[derive(Debug, Serialize)]
struct Pages {
    page: String,
    count: i64,
}

#[derive(Debug, Serialize)]
struct ProfileHits {
    profile_id: Uuid,
    profile_name: String,
    count: i64,
}

#[derive(Debug, Serialize)]
struct UserInfo {
    id: Uuid,
    name: String,
    regdate: i64,
}

#[derive(Debug, Serialize)]
struct FollowerInfo {
    id: Uuid,
    ts: i64,
}
