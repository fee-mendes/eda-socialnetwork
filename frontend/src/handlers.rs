use crate::get_backend;
use askama::Template;
use serde::Serialize;
use chrono::DateTime;
use chrono::NaiveDateTime;
use chrono::Utc;
use axum::extract::Path;
use axum::{
    http::{StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    debug_handler, 
};

pub async fn index() -> impl IntoResponse {
    let template = templates::PageTemplate {
        title: "Home".to_string(),
    };
    HtmlTemplate(template)
}

pub async fn community() -> impl IntoResponse {
    let backend = "http://".to_owned() + &get_backend().await;
    let resp = reqwest::get(backend + "/list").await.unwrap().text().await.unwrap();
    let json: Vec<serde_json::Value> = serde_json::from_str(resp.as_str()).unwrap();

    let mut res: Vec<Users> = vec![];
    for val in json {
        let ts = val["regdate"].to_string().parse::<i64>().unwrap();
        let naive = NaiveDateTime::from_timestamp(ts / 1000, 0);
        let dt: DateTime<Utc> = DateTime::from_utc(naive, Utc);
        let fmt = dt.format("%Y-%m-%d %H:%M");

        let id = val["id"].as_str().unwrap();
        let name = val["name"].as_str().unwrap();

        let i = Users {
            id: id.to_string(),
            name: name.to_string(),
            regdate: fmt.to_string(),
        };
        res.push(i);
    }

    //println!("{:#?}", res);


    let template = templates::CommunityTemplate {
        title: "Community".to_owned(),
        users: res, 
    };
    HtmlTemplate(template)
}

#[debug_handler]
pub async fn profile_view(Path(user_id): Path<String>) -> impl IntoResponse {

    let backend = "http://".to_owned() + &get_backend().await;
    let uri = backend.clone() + "/posts/" + &user_id;
    let user_uri = backend.clone() + "/user/" + &user_id;

    let count_uri = backend + "/user/" + &user_id + "/count";
    let _ = reqwest::get(count_uri).await.unwrap();

    let resp = reqwest::get(uri).await.unwrap().text().await.unwrap();
    let user_resp = reqwest::get(user_uri).await.unwrap().text().await.unwrap();
    let json: Vec<serde_json::Value> = serde_json::from_str(resp.as_str()).unwrap();
    let user_json: serde_json::Value = serde_json::from_str(user_resp.as_str()).unwrap();

    let mut res: Vec<Posts> = vec![];
    for val in json {
        let ts = val["ts"].to_string().parse::<i64>().unwrap();
        let naive = NaiveDateTime::from_timestamp(ts / 1000, 0);
        let dt: DateTime<Utc> = DateTime::from_utc(naive, Utc);
        let fmt = dt.format("%Y-%m-%d %H:%M");

        let subject = val["subject"].as_str().unwrap();
        let content = val["content"].as_str().unwrap();
        let post_id = val["post_id"].as_str().unwrap();

        let i = Posts {
            subject: subject.to_string(),
            content: content.to_string(),
            post_id: post_id.to_string(),
            ts: fmt.to_string(),
        };
        res.push(i)
    }

    let ts = user_json["regdate"].to_string().parse::<i64>().unwrap();
    let naive = NaiveDateTime::from_timestamp(ts / 1000, 0);
    let dt: DateTime<Utc> = DateTime::from_utc(naive, Utc);
    let fmt = dt.format("%Y-%m-%d %H:%M");
    let id = user_json["id"].as_str().unwrap();
    let name = user_json["name"].as_str().unwrap();
    let i = Users {
         id: id.to_string(),
         name: name.to_string(),
         regdate: fmt.to_string(),
    };

    let template = templates::ProfileTemplate {
        title: "View Profile".to_owned(),
        posts: res,
        user: i,
    };
    HtmlTemplate(template)
}

pub async fn followers(Path(user_id): Path<String>) -> impl IntoResponse {

    let backend = "http://".to_owned() + &get_backend().await;
    let follower_uri = backend.clone() + "/followers/" + &user_id;
    let resp = reqwest::get(follower_uri).await.unwrap().text().await.unwrap();
    let json:  Vec<serde_json::Value> = serde_json::from_str(resp.as_str()).unwrap();

    let mut res: Vec<Users> = vec![];
    for val in json {
        let ts = val["regdate"].to_string().parse::<i64>().unwrap();
        let naive = NaiveDateTime::from_timestamp(ts / 1000, 0);
        let dt: DateTime<Utc> = DateTime::from_utc(naive, Utc);
        let fmt = dt.format("%Y-%m-%d %H:%M");

        let id = val["id"].as_str().unwrap();
        let name = val["name"].as_str().unwrap();
        let i = Users {
        id: id.to_string(),
        name: name.to_string(),
        regdate: fmt.to_string(),
        };

        res.push(i)
    }

    let followed_uri = backend + "/followed/" + &user_id;
    let resp = reqwest::get(followed_uri).await.unwrap().text().await.unwrap();
    let json:  Vec<serde_json::Value> = serde_json::from_str(resp.as_str()).unwrap();

    let mut follow_res: Vec<Users> = vec![];
    for val in json {
        let ts = val["regdate"].to_string().parse::<i64>().unwrap();
        let naive = NaiveDateTime::from_timestamp(ts / 1000, 0);
        let dt: DateTime<Utc> = DateTime::from_utc(naive, Utc);
        let fmt = dt.format("%Y-%m-%d %H:%M");

        let id = val["id"].as_str().unwrap();
        let name = val["name"].as_str().unwrap();
        let i = Users {
        id: id.to_string(),
        name: name.to_string(),
        regdate: fmt.to_string(),
        };

        follow_res.push(i)
    }


    let template = templates::FollowerTemplate {
        title: "Followers".to_owned(),
        followers: res,
        followed: follow_res,
    };
    HtmlTemplate(template)
}

pub async fn post() -> impl IntoResponse {
    let template = templates::PostTemplate {
        title: "Speak Up!".to_owned(),
    };
    HtmlTemplate(template)
}

pub async fn redirect_follower() -> impl IntoResponse {
    let template = templates::RedirectFTemplate {
        title: "Followers".to_owned(),
    };
    HtmlTemplate(template)
}

pub async fn fortune() -> impl IntoResponse {
    let template = templates::FortuneTemplate {
        title: "Wheel of Fortune".to_owned(),
    };
    HtmlTemplate(template)
}

pub async fn fortune_profile(Path(id): Path<String>) -> impl IntoResponse {
    let backend = "http://".to_owned() + &get_backend().await;
    let status = reqwest::get(backend.clone() + "/draw/status").await.unwrap().text().await.unwrap();
    let status_json: serde_json::Value = serde_json::from_str(status.as_str()).unwrap();
    let status = status_json["status"].as_bool().unwrap();

    let mut i = GameResult {
        available: status,
        win: false,
    };

    if status {
        let winner = reqwest::get(backend + "/draw/result").await.unwrap().text().await.unwrap();
        let winner_json: serde_json::Value = serde_json::from_str(winner.as_str()).unwrap();
        let winner = winner_json["id"].as_str().unwrap();

        if winner.to_string() == id {
            i = GameResult {
                available: status,
                win: true,
            };
        } 
    }

    let template = templates::FortuneResultTemplate {
        title: "Draw Result".to_owned(),
        status: i,
    };
    HtmlTemplate(template)
}

pub async fn analytics() -> impl IntoResponse {
    let backend = "http://".to_owned() + &get_backend().await;
    let pages = reqwest::get(backend.clone() + "/page_hits").await.unwrap().text().await.unwrap();
    let page_json: Vec<serde_json::Value> = serde_json::from_str(pages.as_str()).unwrap();

    let mut page_res: Vec<Pages> = vec![];
    for val in page_json {
        let page = val["page"].as_str().unwrap();
        let count: i64 = val["count"].as_i64().unwrap();
        let i = Pages {
            page: page.to_string(),
            count: count,
        };

        page_res.push(i);
    }

    let profile_hits = reqwest::get(backend + "/profile_hits").await.unwrap().text().await.unwrap();
    let profile_json: Vec<serde_json::Value> = serde_json::from_str(profile_hits.as_str()).unwrap();

    let mut profile_res: Vec<Profiles> = vec![];
    for val in profile_json {
        let count: i64 = val["count"].as_i64().unwrap();
        let profile_id = val["profile_id"].as_str().unwrap();
        let profile_name = val["profile_name"].as_str().unwrap();

        let i = Profiles {
            name: profile_name.to_string(),
            id: profile_id.to_string(),
            count: count,
        };

        profile_res.push(i);
    }

    let template = templates::AnalyticsTemplate {
        title: "Analytics".to_owned(),
        pages: page_res,
        profiles: profile_res,
    };
    HtmlTemplate(template)
}

pub async fn handle_404(uri: Uri) -> impl IntoResponse {
    let template = templates::NotFoundTemplate {
        title: "404".to_owned(),
        uri: uri.to_string(),
    };
    HtmlTemplate(template)
}

/// Basically all templates handling
pub mod templates {
    use super::*;

    #[derive(Template)]
    #[template(path = "nav-item.html")]
    pub struct PageTemplate {
        pub title: String,
    }

    #[derive(Template)]
    #[template(path = "followers.html")]
    pub struct FollowerTemplate {
        pub title: String,
        pub followers: Vec<Users>,
        pub followed: Vec<Users>,
    }

    #[derive(Template)]
    #[template(path = "redirectfollower.html")]
    pub struct RedirectFTemplate {
        pub title: String,
    }

    #[derive(Template)]
    #[template(path = "analytics.html")]
    pub struct AnalyticsTemplate {
        pub title: String,
        pub pages: Vec<Pages>,
        pub profiles: Vec<Profiles>,
    }


    #[derive(Template)]
    #[template(path = "community.html")]
    pub struct CommunityTemplate {
        pub title: String,
        pub users: Vec<Users>,
    }

    #[derive(Template)]
    #[template(path = "profile.html")]
    pub struct ProfileTemplate {
        pub title: String,
        pub posts: Vec<Posts>,
        pub user: Users
    }

    #[derive(Template)]
    #[template(path = "post.html")]
    pub struct PostTemplate {
        pub title: String,
    }

    #[derive(Template)]
    #[template(path = "fortune.html")]
    pub struct FortuneTemplate {
        pub title: String,
    }

    #[derive(Template)]
    #[template(path = "fortune_result.html")]
    pub struct FortuneResultTemplate {
        pub title: String,
        pub status: GameResult,
    }

    #[derive(Template)]
    #[template(path = "404.html")]
    pub struct NotFoundTemplate {
        pub title: String,
        pub uri: String,
    }
}

#[derive(Serialize, Debug)]
struct Users {
    id: String,
    name: String,
    regdate: String,
}

#[derive(Serialize, Debug)]
struct Posts {
    subject: String,
    content: String,
    ts: String,
    post_id: String,
}

#[derive(Serialize, Debug)]
struct Pages {
    page: String,
    count: i64,
}

#[derive(Serialize, Debug)]
struct Profiles {
    name: String,
    id: String,
    count: i64,
}

#[derive(Serialize, Debug)]
struct GameResult {
    available: bool,
    win: bool,
}

struct HtmlTemplate<T>(T);

impl<T> IntoResponse for HtmlTemplate<T>
where
    T: Template,
{
    fn into_response(self) -> Response {
        match self.0.render() {
            Ok(html) => Html(html).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to render the template. Error: {}", err),
            )
                .into_response(),
        }
    }
}
