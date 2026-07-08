/// Integration tests for the REST API client using wiremock.
use rocketchat::{RestApiClient, RocketChatConfig, ServerConfig, ServerUrl, Username, Password};
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path};

fn test_config(host: &str) -> RocketChatConfig {
    RocketChatConfig {
        server: ServerConfig {
            url: ServerUrl::try_new(host.to_string()).unwrap(),
            username: Username::try_new("bot".into()).unwrap(),
            password: Password::try_new("pw".into()).unwrap(),
            use_tls: false,
        },
    }
}

#[tokio::test]
async fn test_get_rooms_parses_unicode_fname() {
    let server = MockServer::start().await;
    let host = server.address().to_string();

    Mock::given(method("GET"))
        .and(path("/api/v1/rooms.get"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "update": [
                {"_id": "room1", "name": "shit", "fname": "💩💩💩SHIT屎", "t": "p"},
                {"_id": "room2", "name": "general", "fname": "", "t": "c"}
            ],
            "success": true
        })))
        .mount(&server)
        .await;

    let config = test_config(&host);
    let mut client = RestApiClient::new(&config, "uid".into(), "tok".into());
    let rooms = client.get_rooms().await.expect("get_rooms failed");

    assert_eq!(rooms.len(), 2);
    assert_eq!(rooms[0].fname, "💩💩💩SHIT屎");
    assert_eq!(rooms[0].name, "shit");
    assert_eq!(rooms[0].t, "p");
    assert_eq!(rooms[1].name, "general");
}

#[tokio::test]
async fn test_get_room_info_parses_response() {
    let server = MockServer::start().await;
    let host = server.address().to_string();

    Mock::given(method("GET"))
        .and(path("/api/v1/rooms.info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "room": {
                "_id": "room1",
                "name": "shit",
                "fname": "💩💩💩SHIT屎",
                "t": "p"
            },
            "success": true
        })))
        .mount(&server)
        .await;

    let config = test_config(&host);
    let mut client = RestApiClient::new(&config, "uid".into(), "tok".into());
    let room = client
        .get_room_info("room1")
        .await
        .expect("get_room_info failed");

    assert!(room.is_some());
    let room = room.unwrap();
    assert_eq!(room.fname, "💩💩💩SHIT屎");
    assert_eq!(room.name, "shit");
}

#[tokio::test]
async fn test_get_room_info_returns_none_on_404() {
    let server = MockServer::start().await;
    let host = server.address().to_string();

    Mock::given(method("GET"))
        .and(path("/api/v1/rooms.info"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let config = test_config(&host);
    let mut client = RestApiClient::new(&config, "uid".into(), "tok".into());
    let room = client.get_room_info("nonexistent").await.unwrap();
    assert!(room.is_none());
}

#[tokio::test]
async fn test_send_message_with_alias() {
    let server = MockServer::start().await;
    let host = server.address().to_string();

    Mock::given(method("POST"))
        .and(path("/api/v1/chat.sendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "message": {
                "_id": "msg123",
                "rid": "room1",
                "msg": "hello",
                "alias": "MyAlias",
                "u": {"_id": "uid", "username": "bot"}
            },
            "success": true
        })))
        .mount(&server)
        .await;

    let config = test_config(&host);
    let client = RestApiClient::new(&config, "uid".into(), "tok".into());
    let msg_id = client
        .send_message("room1", "hello", Some("MyAlias"))
        .await
        .expect("send_message failed");

    assert_eq!(msg_id, "msg123");
}

#[tokio::test]
async fn test_send_message_without_alias() {
    let server = MockServer::start().await;
    let host = server.address().to_string();

    Mock::given(method("POST"))
        .and(path("/api/v1/chat.sendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "message": {
                "_id": "msg456",
                "rid": "room1",
                "msg": "plain message"
            },
            "success": true
        })))
        .mount(&server)
        .await;

    let config = test_config(&host);
    let client = RestApiClient::new(&config, "uid".into(), "tok".into());
    let msg_id = client
        .send_message("room1", "plain message", None)
        .await
        .expect("send_message failed");

    assert_eq!(msg_id, "msg456");
}

#[tokio::test]
async fn test_send_message_handles_error_response() {
    let server = MockServer::start().await;
    let host = server.address().to_string();

    Mock::given(method("POST"))
        .and(path("/api/v1/chat.sendMessage"))
        .respond_with(ResponseTemplate::new(400).set_body_string("Bad request"))
        .mount(&server)
        .await;

    let config = test_config(&host);
    let client = RestApiClient::new(&config, "uid".into(), "tok".into());
    let result = client
        .send_message("room1", "test", None)
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_resolve_room_fname_caches_result() {
    let server = MockServer::start().await;
    let host = server.address().to_string();

    Mock::given(method("GET"))
        .and(path("/api/v1/rooms.info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "room": {
                "_id": "room1",
                "name": "slug",
                "fname": "中文房名",
                "t": "p"
            },
            "success": true
        })))
        .expect(1) // Should only be called once due to cache
        .mount(&server)
        .await;

    let config = test_config(&host);
    let mut client = RestApiClient::new(&config, "uid".into(), "tok".into());

    // First call: hits the mock
    let fname1 = client.resolve_room_fname("room1").await;
    assert_eq!(fname1, Some("中文房名".to_string()));

    // Second call: should use cache (mock expects only 1 call)
    let fname2 = client.resolve_room_fname("room1").await;
    assert_eq!(fname2, Some("中文房名".to_string()));
}

#[tokio::test]
async fn test_resolve_room_fname_returns_none_for_missing_room() {
    let server = MockServer::start().await;
    let host = server.address().to_string();

    Mock::given(method("GET"))
        .and(path("/api/v1/rooms.info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "room": {},
            "success": false
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/rooms.get"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "update": [],
            "success": true
        })))
        .mount(&server)
        .await;

    let config = test_config(&host);
    let mut client = RestApiClient::new(&config, "uid".into(), "tok".into());
    let fname = client.resolve_room_fname("nonexistent").await;
    assert_eq!(fname, None);
}

#[tokio::test]
async fn test_resolve_room_fname_real_production_shape() {
    let server = MockServer::start().await;
    let host = server.address().to_string();

    // Simulate a room that has roomName="sen1-lin2-sheng1-tai4", fname="🐵🌴🐷森林生態"
    // This is the exact production shape from rc.tokyofy.top
    Mock::given(method("GET"))
        .and(path("/api/v1/rooms.info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "room": {
                "_id": "sPBHTuspPKwPRQdes",
                "name": "sen1-lin2-sheng1-tai4",
                "fname": "🐵🌴🐷森林生態",
                "t": "p"
            },
            "success": true
        })))
        .expect(1)
        .mount(&server)
        .await;

    let config = test_config(&host);
    let mut client = RestApiClient::new(&config, "uid".into(), "tok".into());
    let fname = client.resolve_room_fname("sPBHTuspPKwPRQdes").await;

    assert_eq!(fname, Some("🐵🌴🐷森林生態".to_string()));
}

#[tokio::test]
async fn test_resolve_room_fname_falls_back_to_rooms_get() {
    let server = MockServer::start().await;
    let host = server.address().to_string();

    // rooms.info returns success=false (no fname known)
    Mock::given(method("GET"))
        .and(path("/api/v1/rooms.info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "room": {
                "_id": "target_room",
                "name": "target",
                "fname": "",
                "t": "p"
            },
            "success": true
        })))
        .mount(&server)
        .await;

    // rooms.get has the target room with the actual fname
    Mock::given(method("GET"))
        .and(path("/api/v1/rooms.get"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "update": [
                {"_id": "other_room", "name": "general", "fname": "", "t": "c"},
                {"_id": "target_room", "name": "target", "fname": "實際中文名", "t": "p"}
            ],
            "success": true
        })))
        .mount(&server)
        .await;

    let config = test_config(&host);
    let mut client = RestApiClient::new(&config, "uid".into(), "tok".into());
    let fname = client.resolve_room_fname("target_room").await;

    assert_eq!(fname, Some("實際中文名".to_string()));
}

#[tokio::test]
async fn test_resolve_room_fname_rooms_info_empty_fname_falls_through() {
    let server = MockServer::start().await;
    let host = server.address().to_string();

    // rooms.info returns success with empty fname (e.g. #general has no display name)
    Mock::given(method("GET"))
        .and(path("/api/v1/rooms.info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "room": {
                "_id": "general_room",
                "name": "general",
                "fname": "",
                "t": "c"
            },
            "success": true
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/rooms.get"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "update": [
                {"_id": "general_room", "name": "general", "fname": "", "t": "c"}
            ],
            "success": true
        })))
        .mount(&server)
        .await;

    let config = test_config(&host);
    let mut client = RestApiClient::new(&config, "uid".into(), "tok".into());
    let fname = client.resolve_room_fname("general_room").await;

    // Both endpoints returned empty fname — falls through to None
    assert_eq!(fname, None);
}

#[tokio::test]
async fn test_get_message_parses_response() {
    let server = MockServer::start().await;
    let host = server.address().to_string();

    Mock::given(method("GET"))
        .and(path("/api/v1/chat.getMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "message": {
                "_id": "msg1",
                "msg": "hello",
                "alias": "零夢✨"
            },
            "success": true
        })))
        .mount(&server)
        .await;

    let config = test_config(&host);
    let client = RestApiClient::new(&config, "uid".into(), "tok".into());
    let msg = client
        .get_message("msg1")
        .await
        .expect("get_message failed")
        .expect("message should exist");

    assert_eq!(msg["alias"], "零夢✨");
    assert_eq!(msg["msg"], "hello");
}
