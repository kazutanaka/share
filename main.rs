use aws_sdk_dynamodb::{Client, types::AttributeValue};
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc, SecondsFormat};
use uuid::Uuid;
use serde_dynamo::aws_sdk_dynamodb_1::{from_item, from_items, to_item};

const SESSION_TABLE_NAME: &str = "atok_sync_one-beta-sessions";
#[allow(dead_code)]
const HISTORY_TABLE_NAME: &str = "atok_sync_one-beta-histories";
const SESSION_PK_NAME: &str = "serial_hash";
const SESSION_SK_NAME: &str = "session_id";
const SESSION_CLOSED_NAME: &str = "closed";
const SESSION_EXPIRE_AT_NAME: &str = "expire_at";

#[derive(Debug, Serialize, Deserialize)]
pub struct DynamoSession {
    pub serial_hash: String, // pk
    pub session_id: String,  // sk
    pub timestamp_id: String,
    pub serial: String,
    pub auth_user: String,
    pub token: String,
    pub internalid: String,
    pub device: Option<String>,
    pub display_name: String,
    pub target: Option<String>,
    pub base_syncid: Option<String>,
    pub merge_mode: Option<String>,
    pub merge_mode_reason: Option<String>,
    pub merge_status: Option<String>,
    pub merge_error: Option<String>,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub started_at: DateTime<Utc>,

    #[serde(with = "chrono::serde::ts_seconds_option")]
    pub completed_at: Option<DateTime<Utc>>,

    pub closed: bool,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub expire_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DynamoHistory {
    pub serial_hash_sync_target: String, // pk
    pub timestamp_id: String,
    pub serial: String,
    pub internalid: String,
    pub device: Option<String>,
    pub display_name: String,
    pub base_syncid: Option<String>,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub created_at: DateTime<Utc>,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub expire_at: DateTime<Utc>,
}

#[derive(Debug)]
struct SessionId(pub String);

#[derive(Serialize, Deserialize)]
struct History {
    pub syncid: String,
    pub target: String,
    pub created_at: String,
    pub device: Device,
}

#[derive(Serialize, Deserialize)]
struct Device {
    display_name: String,
    id: Option<String>,
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // AWS Config for DynamoDB Local Credential and Endpoint.
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .test_credentials()
        .endpoint_url("http://localhost:8000")
        .load()
        .await;

    // DynamoDB Client for DynamoDB Local
    let client = Client::new(&config);

    // セッションID
    let serial_hash = "sample_serial_hash";

    // セッション数 -- query
    let sessions = query_sessions(&client, serial_hash).await?;
    println!("serial_hash {} forward match: {}", serial_hash, sessions.len());

    // セッション生成
    let session_id = create_session(&client, serial_hash, "1234567890", "dummy_user", "dummy_token", "dummy_internalid", "不明", 1).await?;
    println!("created session: {}", session_id.0);

    // セッション検索 -- get_item
    match get_session(&client, serial_hash, &session_id).await? {
        Some(session) => {
            println!("Item found for serial_hash {}, session_id {}", serial_hash, session_id.0);
            println!("timestamp_id: {}", session.timestamp_id);
            println!("serial: {}", session.serial);
            println!("auth_user: {}", session.auth_user);
            println!("token: {}", session.token);
            println!("internalid: {}", session.internalid);
            println!("device: {:?}", session.device);
            println!("display_name: {}", session.display_name);
            println!("target: {:?}", session.target);
            println!("base_syncid: {:?}", session.base_syncid);
            println!("merge_mode: {:?}", session.merge_mode);
            println!("merge_mode_reason: {:?}", session.merge_mode_reason);
            println!("merge_status: {:?}", session.merge_status);
            println!("merge_error: {:?}", session.merge_error);
            println!("started_at: {}", to_rfc3339(&session.started_at));
            println!("completed_at: {}", session.completed_at.map(|dt| to_rfc3339(&dt)).unwrap_or_else(|| "None".to_string()));
            println!("closed: {}", session.closed);
            println!("expire_at: {}", to_rfc3339(&session.expire_at));
        },
        None => {
            println!("No item matched for serial_hash {}, session_id {}", serial_hash, session_id.0);
        },
    }

    // セッション生成後のセッション数
    let sessions = query_sessions(&client, serial_hash).await?;
    println!("after creating session, count: {}", sessions.len());

    // セッション終了
    let flag = rand::random::<bool>();
    if flag {
        close_session(&client, serial_hash, &session_id).await?;
    }

    // 50%でセッション終了後のセッション数
    let sessions = query_sessions(&client, serial_hash).await?;
    println!("after closing session, count: {}", sessions.len());

    let data = vec![
        History {
            syncid: "syncid-1".to_string(),
            target: "target-1".to_string(),
            created_at: "created_at-1".to_string(),
            device: Device {
                display_name: "display_name-1".to_string(),
                id: Some("id-1".to_string()),
            },
        },
        History {
            syncid: "syncid-2".to_string(),
            target: "target-2".to_string(),
            created_at: "created_at-2".to_string(),
            device: Device {
                display_name: "display_name-2".to_string(),
                id: None,
            },
        },
    ];

    let json = serde_json::to_string_pretty(&data)?;

    println!("json: {}", json);

    Ok(())
}

// シリアルハッシュの発行セッションクエリ
async fn query_sessions(client: &Client, serial_hash: &str) -> Result<Vec<DynamoSession>, Box<dyn std::error::Error>> {
    let now = Utc::now().timestamp().to_string();
    let results = client
        .query()
        .table_name(SESSION_TABLE_NAME)
        .key_condition_expression("#pk = :pk_val")
        .filter_expression("#closed = :closed_val AND #expire_at > :expire_at_val")
        .expression_attribute_names("#pk", SESSION_PK_NAME)
        .expression_attribute_names("#closed", SESSION_CLOSED_NAME)
        .expression_attribute_names("#expire_at", SESSION_EXPIRE_AT_NAME)
        .expression_attribute_values(":pk_val", AttributeValue::S(serial_hash.to_string()))
        .expression_attribute_values(":closed_val", AttributeValue::Bool(false))
        .expression_attribute_values(":expire_at_val", AttributeValue::N(now))
        .send()
        .await?;

    if let Some(items) = results.items {
        Ok(from_items(items)?)
    } else {
        Ok(Vec::new())
    }

}

// セッション検索
async fn get_session(client: &Client, serial_hash: &str, session_id: &SessionId) -> Result<Option<DynamoSession>, Box<dyn std::error::Error>> {
    let response = client
        .get_item()
        .table_name(SESSION_TABLE_NAME)
        .key(SESSION_PK_NAME, AttributeValue::S(serial_hash.to_string()))
        .key(SESSION_SK_NAME, AttributeValue::S(session_id.0.clone()))
        .send()
        .await?;

    if let Some(item) = response.item {
        let session: DynamoSession = from_item(item).unwrap();
        Ok(Some(session))
    } else {
        Ok(None)
    }

}

// セッション生成
async fn create_session(client: &Client, serial_hash: &str, serial: &str, auth_user: &str, token: &str, internalid: &str, display_name: &str, expiry_minutes: i64) -> Result<SessionId, Box<dyn std::error::Error>> {
    let session_id = Uuid::new_v4().to_string();

    let session = DynamoSession {
        serial_hash: serial_hash.to_string(),
        session_id: session_id.clone(),
        timestamp_id: Uuid::new_v4().to_string(),
        serial: serial.to_string(),
        auth_user: auth_user.to_string(),
        token: token.to_string(),
        internalid: internalid.to_string(),
        device: None,
        display_name: display_name.to_string(),
        target: None,
        base_syncid: None,
        merge_mode: None,
        merge_mode_reason: None,
        merge_status: None,
        merge_error: None,
        started_at: Utc::now(),
        completed_at: None,
        closed: false,
        expire_at:Utc::now() + chrono::Duration::minutes(expiry_minutes),
    };

    // セッション登録 -- put_item
    client
        .put_item()
        .table_name(SESSION_TABLE_NAME)
        .set_item(Some(to_item(session).unwrap()))
        .send()
        .await?;

    Ok(SessionId(session_id))
}

// セッション終了
async fn close_session(client: &Client, serial_hash: &str, session_id: &SessionId) -> Result<(), Box<dyn std::error::Error>> {
    client
        .update_item()
        .table_name(SESSION_TABLE_NAME)
        .key(SESSION_PK_NAME, AttributeValue::S(serial_hash.to_string()))
        .key(SESSION_SK_NAME, AttributeValue::S(session_id.0.clone()))
        .update_expression("SET #closed = :closed_val")
        .expression_attribute_names("#closed", SESSION_CLOSED_NAME)
        .expression_attribute_values(":closed_val", AttributeValue::Bool(true))
        .send()
        .await?;

    Ok(())
}

// iso8609
fn to_rfc3339(datetime: &DateTime<Utc>) -> String {
    datetime.to_rfc3339_opts(SecondsFormat::Secs, true)
}

