//! Two players through the whole contract: accounts, armies (imported and
//! editor-built), a lobby, the start, and the WebSocket protocol.

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use content::army_def::{self, ArmyDef};
use content::catalogue::Catalogue;
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tower::ServiceExt;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// A server on an ephemeral port plus the same router for in-process calls.
struct TestServer {
    app: Router,
    addr: SocketAddr,
    db_path: PathBuf,
}

impl TestServer {
    async fn start(tag: &str) -> TestServer {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let db_path = std::env::temp_dir().join(format!("grimtable-test-{tag}-{}-{nanos}.db", std::process::id()));
        let state = server::AppState::new(&db_path, root()).unwrap();
        let app = server::router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let serve_app = app.clone();
        tokio::spawn(async move { axum::serve(listener, serve_app).await.unwrap() });
        TestServer { app, addr, db_path }
    }

    fn client(&self) -> Client {
        Client { app: self.app.clone(), cookie: None }
    }

    async fn ws(&self, client: &Client, game: i64) -> Ws {
        let mut req = format!("ws://{}/api/games/{game}/ws", self.addr).into_client_request().unwrap();
        req.headers_mut().insert(header::COOKIE, client.cookie.clone().unwrap().parse().unwrap());
        let (ws, _) = tokio_tungstenite::connect_async(req).await.expect("websocket upgrade");
        ws
    }

    fn actions_logged(&self, game: i64) -> i64 {
        let conn = rusqlite::Connection::open(&self.db_path).unwrap();
        conn.query_row("SELECT count(*) FROM game_actions WHERE game_id = ?1", [game], |r| r.get(0)).unwrap()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.db_path.display()));
        }
    }
}

/// One user's cookie jar over the in-process router.
#[derive(Clone)]
struct Client {
    app: Router,
    cookie: Option<String>,
}

impl Client {
    async fn call(&mut self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, c);
        }
        let req = match body {
            Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())).unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        if let Some(set) = resp.headers().get(header::SET_COOKIE) {
            let pair = set.to_str().unwrap().split(';').next().unwrap().to_string();
            self.cookie = if pair.ends_with('=') { None } else { Some(pair) };
        }
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let value = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into())) };
        (status, value)
    }

    async fn get(&mut self, path: &str) -> (StatusCode, Value) {
        self.call(Method::GET, path, None).await
    }

    async fn post(&mut self, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(Method::POST, path, Some(body)).await
    }

    async fn register(&mut self, name: &str) -> i64 {
        let (status, body) = self.post("/api/register", json!({ "username": name, "password": "hunter2" })).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["username"], name);
        body["id"].as_i64().unwrap()
    }

    async fn import(&mut self, name: &str, fixture: &str) -> Value {
        let xml = std::fs::read_to_string(root().join("crates/roster/fixtures").join(fixture)).unwrap();
        let (status, body) = self.post("/api/armies/import", json!({ "name": name, "ros": xml })).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["kind"], "ros");
        body
    }
}

async fn recv(ws: &mut Ws) -> Value {
    let msg = tokio::time::timeout(Duration::from_secs(20), ws.next()).await.expect("socket message in time").expect("socket open").unwrap();
    match msg {
        Message::Text(t) => serde_json::from_str(&t).unwrap(),
        other => panic!("unexpected frame {other:?}"),
    }
}

async fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::Text(v.to_string().into())).await.unwrap();
}

/// A small, legal editor army from the Mechanicus catalogue.
fn editor_army(cat: &Catalogue) -> ArmyDef {
    let unit = |id: &str| army_def::default_unit(cat.units.iter().find(|u| u.id == id).unwrap_or_else(|| panic!("{id}")));
    let mut cawl = unit("belisarius-cawl");
    cawl.warlord = true;
    ArmyDef {
        name: "Forge Detail".into(),
        faction_id: "adeptus-mechanicus".into(),
        detachment_ids: vec!["lords-of-the-forge".into()],
        units: vec![cawl, unit("tech-priest-dominus"), unit("skitarii-rangers"), unit("kataphron-breachers")],
    }
}

#[tokio::test]
async fn two_players_play_over_websockets() {
    let server = TestServer::start("play").await;
    let mut alice = server.client();
    let mut bob = server.client();

    // Accounts.
    let (status, _) = alice.get("/api/me").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let alice_id = alice.register("alice").await;
    bob.register("bob").await;
    let (status, me) = alice.get("/api/me").await;
    assert_eq!((status, me["id"].as_i64().unwrap()), (StatusCode::OK, alice_id));
    let (status, _) = server.client().post("/api/register", json!({ "username": "alice", "password": "x" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = server.client().post("/api/login", json!({ "username": "alice", "password": "wrong" })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let mut alice_again = server.client();
    let (status, _) = alice_again.post("/api/login", json!({ "username": "alice", "password": "hunter2" })).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = alice_again.post("/api/logout", json!({})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(alice_again.get("/api/me").await.0, StatusCode::UNAUTHORIZED);

    // Imported armies: scrubbed, faction resolved, points recomputed.
    let mech = alice.import("Mechanicus", "mechanicus.ros").await;
    assert_eq!(mech["faction_id"], "adeptus-mechanicus");
    assert_eq!(mech["points"], 1980);
    assert_eq!(mech["problems"], json!([]), "{}", mech["problems"]);
    let necrons = bob.import("Necrons", "necrons.ros").await;
    assert_eq!(necrons["points"], 2055);
    assert!(necrons["problems"].as_array().unwrap().iter().any(|p| p.as_str().unwrap().contains("over the 2000 pt limit")), "{}", necrons["problems"]);
    let (status, list) = bob.get("/api/armies").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(bob.get(&format!("/api/armies/{}", mech["id"])).await.0, StatusCode::NOT_FOUND, "armies are private");

    // Editor army from the catalogue.
    let (status, factions) = bob.get("/api/catalogue").await;
    assert_eq!(status, StatusCode::OK);
    assert!(factions.as_array().unwrap().iter().any(|f| f["faction_id"] == "adeptus-mechanicus"));
    let (status, cat) = bob.get("/api/catalogue/adeptus-mechanicus").await;
    assert_eq!(status, StatusCode::OK);
    let cat: Catalogue = serde_json::from_value(cat).unwrap();
    let def = editor_army(&cat);
    let (status, army) = bob.post("/api/armies", json!({ "name": "Forge Detail", "kind": "def", "def": def })).await;
    assert_eq!(status, StatusCode::CREATED, "{army}");
    assert_eq!(army["kind"], "def");
    assert_eq!(army["faction_id"], "adeptus-mechanicus");
    assert_eq!(army["problems"], json!([]), "{}", army["problems"]);
    assert_eq!(army["points"], army_def::total_points(&def, &cat));
    let def_id = army["id"].as_i64().unwrap();
    let (status, pack) = bob.get(&format!("/api/armies/{def_id}/pack")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pack["units"].as_array().unwrap().len(), 4);
    assert!(pack["units"][0]["keywords"].as_array().unwrap().iter().any(|k| k == "Warlord"));
    let (status, renamed) = bob.call(Method::PUT, &format!("/api/armies/{def_id}"), Some(json!({ "name": "Forge Detail II" }))).await;
    assert_eq!((status, renamed["name"].as_str().unwrap()), (StatusCode::OK, "Forge Detail II"));

    // Closed lobby: alice hosts, bob joins by code.
    let (status, game) = alice.post("/api/games", json!({ "visibility": "closed", "pattern": "dawn-of-war", "points_limit": 2000, "army_id": mech["id"] })).await;
    assert_eq!(status, StatusCode::CREATED, "{game}");
    let game_id = game["id"].as_i64().unwrap();
    let code = game["join_code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 6);
    assert!(code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
    assert_eq!((game["status"].as_str().unwrap(), game["my_seat"].as_i64().unwrap()), ("waiting", 0));
    assert_eq!(alice.post(&format!("/api/games/{game_id}/start"), json!({})).await.0, StatusCode::CONFLICT, "needs a second player");
    assert_eq!(bob.post("/api/games/join", json!({ "join_code": "ZZZZZZ", "army_id": def_id })).await.0, StatusCode::NOT_FOUND);
    let (status, joined) = bob.post("/api/games/join", json!({ "join_code": code, "army_id": def_id })).await;
    assert_eq!(status, StatusCode::OK, "{joined}");
    assert_eq!(joined["my_seat"], 1);
    assert_eq!(joined["seats"].as_array().unwrap().len(), 2);
    assert_eq!(joined["seats"][1]["username"], "bob");
    assert_eq!(bob.post(&format!("/api/games/{game_id}/start"), json!({})).await.0, StatusCode::FORBIDDEN, "host only");

    let (status, started) = alice.post(&format!("/api/games/{game_id}/start"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["status"], "playing");
    assert_eq!(started["problems"], json!([[], []]));
    let (_, shown) = bob.get(&format!("/api/games/{game_id}")).await;
    assert_eq!((shown["status"].as_str().unwrap(), shown["my_seat"].as_i64().unwrap()), ("playing", 1));
    let (_, mine) = bob.get("/api/games/mine").await;
    assert_eq!(mine.as_array().unwrap().len(), 1);

    // Sockets: state on connect, refused action only to its sender,
    // accepted action to everyone, including a spectator.
    let mut carol = server.client();
    carol.register("carol").await;
    let mut ws_a = server.ws(&alice, game_id).await;
    let mut ws_b = server.ws(&bob, game_id).await;
    let mut ws_c = server.ws(&carol, game_id).await;
    let first_a = recv(&mut ws_a).await;
    assert_eq!((first_a["type"].as_str().unwrap(), first_a["seat"].as_i64().unwrap(), first_a["to_act"].as_i64().unwrap()), ("state", 0, 0));
    assert_eq!(first_a["state"]["phase"], "Muster");
    assert_eq!(first_a["state"]["players"][1]["name"], "Forge Detail");
    let first_b = recv(&mut ws_b).await;
    assert_eq!((first_b["type"].as_str().unwrap(), first_b["seat"].as_i64().unwrap()), ("state", 1));
    let first_c = recv(&mut ws_c).await;
    assert_eq!((first_c["type"].as_str().unwrap(), first_c["seat"].clone()), ("state", Value::Null));

    send(&mut ws_c, json!({ "type": "ping" })).await;
    assert_eq!(recv(&mut ws_c).await["type"], "pong");
    send(&mut ws_c, json!({ "type": "action", "action": { "type": "finish_muster" } })).await;
    let refused = recv(&mut ws_c).await;
    assert_eq!((refused["type"].as_str().unwrap(), refused["message"].as_str().unwrap()), ("error", "spectators cannot act"));

    send(&mut ws_b, json!({ "type": "action", "action": { "type": "end_phase" } })).await;
    let refused = recv(&mut ws_b).await;
    assert_eq!(refused["type"], "error");
    assert_eq!(refused["message"], "not your turn");
    assert_eq!(server.actions_logged(game_id), 0);

    // Alice finishes mustering: everyone sees seat 0 marked done.
    send(&mut ws_a, json!({ "type": "action", "action": { "type": "finish_muster" } })).await;
    for ws in [&mut ws_a, &mut ws_b, &mut ws_c] {
        let frame = recv(ws).await;
        assert_eq!(frame["type"], "state", "{frame}");
        assert_eq!(frame["state"]["phase"], "Muster");
        assert_eq!(frame["state"]["fight_queue"], json!([0]), "muster-done list");
    }
    assert_eq!(server.actions_logged(game_id), 1);
    // Bob too: the engine rolls off and deployment begins for the winner.
    send(&mut ws_b, json!({ "type": "action", "action": { "type": "finish_muster" } })).await;
    for ws in [&mut ws_a, &mut ws_b, &mut ws_c] {
        let frame = recv(ws).await;
        assert_eq!(frame["type"], "state", "{frame}");
        assert_eq!(frame["state"]["phase"], "Deployment");
        assert!(frame["state"]["log"].as_array().unwrap().iter().any(|e| e["text"].as_str().unwrap().starts_with("Roll-off")), "{}", frame["state"]["log"]);
        assert_eq!(frame["to_act"], frame["state"]["first_player"]);
    }
    assert_eq!(server.actions_logged(game_id), 2);
    let (_, shown) = alice.get(&format!("/api/games/{game_id}")).await;
    assert_eq!(shown["status"], "playing");
    assert_eq!(alice.call(Method::DELETE, &format!("/api/armies/{}", mech["id"]), None).await.0, StatusCode::CONFLICT, "army in a live game");
}

#[tokio::test]
async fn open_lobby_broadcasts_and_refuses_an_illegal_army() {
    let server = TestServer::start("lobby").await;
    let mut alice = server.client();
    let mut bob = server.client();
    alice.register("alice").await;
    bob.register("bob").await;
    let mech = alice.import("Mechanicus", "mechanicus.ros").await;
    let necrons = bob.import("Necrons", "necrons.ros").await;

    let (status, game) = alice.post("/api/games", json!({ "visibility": "open", "pattern": "dawn-of-war", "points_limit": 2000, "army_id": mech["id"] })).await;
    assert_eq!(status, StatusCode::CREATED, "{game}");
    let game_id = game["id"].as_i64().unwrap();
    let (_, open) = bob.get("/api/games/open").await;
    let listed = open.as_array().unwrap().iter().find(|g| g["id"] == game_id).expect("open game listed");
    assert_eq!(listed["host"], "alice");

    // The host's socket sees the lobby, then the join.
    let mut ws_a = server.ws(&alice, game_id).await;
    let lobby = recv(&mut ws_a).await;
    assert_eq!(lobby["type"], "lobby");
    assert_eq!(lobby["game"]["seats"].as_array().unwrap().len(), 1);
    let (status, joined) = bob.post(&format!("/api/games/{game_id}/join"), json!({ "army_id": necrons["id"] })).await;
    assert_eq!(status, StatusCode::OK, "{joined}");
    let lobby = recv(&mut ws_a).await;
    assert_eq!(lobby["type"], "lobby");
    assert_eq!(lobby["game"]["seats"][1]["username"], "bob");
    assert_eq!(lobby["game"]["my_seat"], 0);
    assert!(bob.get("/api/games/open").await.1.as_array().unwrap().iter().all(|g| g["id"] != game_id), "full lobby is no longer open");

    // Over the points limit: refused, with the problems per seat.
    let (status, refused) = alice.post(&format!("/api/games/{game_id}/start"), json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["problems"][0], json!([]));
    assert!(refused["problems"][1].as_array().unwrap().iter().any(|p| p.as_str().unwrap().contains("over the 2000 pt limit")), "{refused}");
    let (_, shown) = alice.get(&format!("/api/games/{game_id}")).await;
    assert_eq!(shown["status"], "waiting");
}
