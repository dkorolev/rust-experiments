use axum::{
  Router,
  extract::{Query, State},
  routing::get,
  serve,
};
use futures::{SinkExt, StreamExt};
use hyper::StatusCode;
use redb::{Database, ReadableTable, TableDefinition};
use redis::{AsyncCommands, RedisResult};
use serde::{Deserialize, Serialize};
use std::{any::Any, error::Error, fs, net::SocketAddr, path::Path};
use tokio::{
  signal::unix::{SignalKind, signal},
  sync::{mpsc, oneshot},
  time::{Duration, sleep},
};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use url::Url;

use anyhow::Result;
use chrono::Local;
use clap::Parser;

const DEBUG_WS_MESSAGES: bool = true;

#[derive(Parser)]
struct Args {
  #[arg(long, default_value = "redis://127.0.0.1")]
  redis: String,

  #[arg(long, default_value = "wss://ws.kraken.com/")]
  ws_host: String,

  #[arg(long, default_value = r#"{"event": "subscribe", "pair": ["XBT/USD"], "subscription": {"name": "trade"}}"#)]
  ws_subscribe_cmd: String,

  #[arg(long, default_value = "3000")]
  port: u16,
}

const MAX_PAGE_SIZE: u64 = 100;

static TRADES: TableDefinition<u64, &[u8]> = TableDefinition::new("trades");
static TRADE_IDS: TableDefinition<&str, u64> = TableDefinition::new("trade_ids");
static TRADES_META: TableDefinition<&str, u64> = TableDefinition::new("trades_meta");

pub type AsyncDbResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const DEFAULT_REQUEST_QUEUE_SIZE: usize = 32;

pub trait DbRequestHandler: Send + Sync {
  type Response: Send + 'static;
  fn handle(&self, db: &Database) -> AsyncDbResult<Self::Response>;
}

trait GenericRequestHandler: Send + Sync {
  fn handle_generic_request(&self, db: &Database) -> AsyncDbResult<Box<dyn Any + Send>>;
}

impl<T: DbRequestHandler> GenericRequestHandler for T {
  fn handle_generic_request(&self, db: &Database) -> AsyncDbResult<Box<dyn Any + Send>> {
    self.handle(db).map(|v| Box::new(v) as Box<dyn Any + Send>)
  }
}

struct GenericRequest {
  request: Box<dyn GenericRequestHandler + Send + Sync>,
  response_tx: oneshot::Sender<AsyncDbResult<Box<dyn Any + Send>>>,
}

#[derive(Clone)]
pub struct AsyncRedb {
  request_tx: mpsc::Sender<GenericRequest>,
}

impl AsyncRedb {
  pub fn new(db: Database) -> Self {
    Self::with_queue_size(db, DEFAULT_REQUEST_QUEUE_SIZE)
  }

  pub fn with_queue_size(db: Database, queue_size: usize) -> Self {
    let (request_tx, mut request_rx): (mpsc::Sender<GenericRequest>, mpsc::Receiver<GenericRequest>) =
      mpsc::channel(queue_size);

    std::thread::spawn(move || {
      while let Some(request) = request_rx.blocking_recv() {
        let result = request.request.handle_generic_request(&db);
        let _ = request.response_tx.send(result);
      }
    });

    Self { request_tx }
  }

  pub async fn run<R>(&self, request: R) -> AsyncDbResult<R::Response>
  where
    R: DbRequestHandler + Send + Sync + 'static,
  {
    let (response_tx, response_rx) = oneshot::channel();

    let request = GenericRequest { request: Box::new(request), response_tx };

    self.request_tx.send(request).await?;

    let boxed_result = response_rx.await.map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)??;
    boxed_result.downcast::<R::Response>().map(|b| *b).map_err(|_| {
      Box::new(std::io::Error::new(std::io::ErrorKind::Other, "type mismatch")) as Box<dyn Error + Send + Sync>
    })
  }
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum KrakenMessage {
  TradeMessage(KrakenTradeMessage),
  EventMessage(KrakenEventMessage),
  Generic(serde_json::Value),
}

#[derive(Deserialize, Debug)]
#[serde(tag = "event")]
enum KrakenEventMessage {
  #[serde(rename = "systemStatus")]
  SystemStatus(SystemStatus),
  #[serde(rename = "subscriptionStatus")]
  SubscriptionStatus(SubscriptionStatus),
  #[serde(rename = "heartbeat")]
  Heartbeat,
  #[serde(other)]
  Unknown,
}

#[derive(Deserialize, Debug)]
struct SystemStatus {
  #[allow(dead_code)]
  event: String,
  #[allow(dead_code)]
  version: String,
  status: String,
  #[serde(rename = "connectionID")]
  connection_id: u64,
}

#[derive(Deserialize, Debug)]
struct SubscriptionStatus {
  #[serde(rename = "channelID")]
  #[allow(dead_code)]
  channel_id: u64,
  #[serde(rename = "channelName")]
  channel_name: String,
  #[allow(dead_code)]
  event: String,
  pair: String,
  status: String,
  #[allow(dead_code)]
  subscription: Subscription,
}

#[derive(Deserialize, Debug)]
struct Subscription {
  #[allow(dead_code)]
  name: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct KrakenSubscribeMessage {
  event: String,
  pair: Vec<String>,
  subscription: KrakenSubscriptionDetails,
}

#[derive(Serialize, Deserialize, Debug)]
struct KrakenSubscriptionDetails {
  name: String,
}

#[derive(Deserialize, Debug)]
struct KrakenTradeMessage(u64, Vec<Vec<JsonValue>>, String, String);

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
enum JsonValue {
  String(String),
  Number(f64),
  Bool(bool),
  Null,
  Array(()),
  Object(()),
}

impl JsonValue {
  fn to_string(&self) -> String {
    match self {
      JsonValue::String(s) => s.clone(),
      JsonValue::Number(n) => n.to_string(),
      JsonValue::Bool(b) => b.to_string(),
      JsonValue::Null => "null".to_string(),
      JsonValue::Array(_) => "[array]".to_string(),
      JsonValue::Object(_) => "{object}".to_string(),
    }
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Trade {
  id: String,
  price: String,
  volume: String,
  timestamp: String,
  side: String,
  order_type: String,
  misc: String,
  pair: String,
}

struct StoreTradeBatchRequest(Vec<(Trade, u64)>);

impl DbRequestHandler for StoreTradeBatchRequest {
  type Response = u64;

  fn handle(&self, db: &Database) -> AsyncDbResult<Self::Response> {
    let write_txn = db.begin_write()?;
    let stored_count;
    {
      let mut trades = write_txn.open_table(TRADES)?;
      let mut trade_ids = write_txn.open_table(TRADE_IDS)?;

      for (trade, index) in &self.0 {
        let serialized = serde_json::to_vec(trade)?;
        trades.insert(*index, serialized.as_slice())?;
        trade_ids.insert(trade.id.as_str(), *index)?;
      }

      stored_count = self.0.len() as u64;
    }
    write_txn.commit()?;
    Ok(stored_count)
  }
}

struct GetTotalTradesCountRequest;

impl DbRequestHandler for GetTotalTradesCountRequest {
  type Response = u64;

  fn handle(&self, db: &Database) -> AsyncDbResult<Self::Response> {
    let read_txn = db.begin_read()?;
    let count;
    {
      let trades_meta = read_txn.open_table(TRADES_META)?;
      count = trades_meta.get("total")?.map(|v| v.value()).unwrap_or(0);
    }
    Ok(count)
  }
}

struct GetRecentTradesRequest(u64);

impl DbRequestHandler for GetRecentTradesRequest {
  type Response = Vec<Trade>;

  fn handle(&self, db: &Database) -> AsyncDbResult<Self::Response> {
    let read_txn = db.begin_read()?;
    let trades_meta = read_txn.open_table(TRADES_META)?;
    let trades = read_txn.open_table(TRADES)?;

    let total = trades_meta.get("total")?.map(|v| v.value()).unwrap_or(0);

    let mut result = Vec::new();
    let n = std::cmp::min(std::cmp::min(self.0, total), MAX_PAGE_SIZE);

    if n == 0 {
      return Ok(result);
    }

    for i in (total - n + 1)..=total {
      if let Some(entry_bytes) = trades.get(i)? {
        if let Ok(trade) = serde_json::from_slice::<Trade>(entry_bytes.value()) {
          result.push(trade);
        }
      }
    }

    Ok(result)
  }
}

#[derive(Deserialize)]
struct TradeQueryParams {
  start_index: Option<u64>,
  limit: Option<u64>,
}

struct GetTradesByRangeRequest {
  start_index: u64,
  limit: u64,
}

impl DbRequestHandler for GetTradesByRangeRequest {
  type Response = Vec<Trade>;

  fn handle(&self, db: &Database) -> AsyncDbResult<Self::Response> {
    let read_txn = db.begin_read()?;
    let trades = read_txn.open_table(TRADES)?;

    let mut result = Vec::new();
    let end_index = self.start_index + self.limit;

    for i in self.start_index..end_index {
      if let Some(entry_bytes) = trades.get(i)? {
        if let Ok(trade) = serde_json::from_slice::<Trade>(entry_bytes.value()) {
          result.push(trade);
        }
      } else {
        break;
      }
    }

    Ok(result)
  }
}

#[derive(Clone)]
struct AppState {
  redb: AsyncRedb,
}

struct GetNextTradeCounterRequest;

impl DbRequestHandler for GetNextTradeCounterRequest {
  type Response = u64;

  fn handle(&self, db: &Database) -> AsyncDbResult<Self::Response> {
    let current_total;
    {
      let read_txn = db.begin_read()?;
      let trades_meta = read_txn.open_table(TRADES_META)?;
      current_total = trades_meta.get("total")?.map(|v| v.value()).unwrap_or(0);
    }

    let new_total = current_total + 1;
    let write_txn = db.begin_write()?;
    {
      let mut trades_meta = write_txn.open_table(TRADES_META)?;
      trades_meta.insert("total", new_total)?;
    }
    write_txn.commit()?;

    Ok(new_total)
  }
}

struct CheckIdExistsRequest(String);

impl DbRequestHandler for CheckIdExistsRequest {
  type Response = bool;

  fn handle(&self, db: &Database) -> AsyncDbResult<Self::Response> {
    let read_txn = db.begin_read()?;
    let exists;
    {
      let trade_ids = read_txn.open_table(TRADE_IDS)?;
      exists = trade_ids.get(self.0.as_str())?.is_some();
    }
    Ok(exists)
  }
}

struct StoreIdWithIndexRequest {
  id: String,
  index: u64,
}

impl DbRequestHandler for StoreIdWithIndexRequest {
  type Response = ();

  fn handle(&self, db: &Database) -> AsyncDbResult<Self::Response> {
    let write_txn = db.begin_write()?;
    {
      let mut trade_ids = write_txn.open_table(TRADE_IDS)?;
      trade_ids.insert(self.id.as_str(), self.index)?;
    }
    write_txn.commit()?;
    Ok(())
  }
}

fn log_message_format(message_type: &str, text: &str) {
  if !DEBUG_WS_MESSAGES {
    return;
  }

  let truncated = if text.len() > 500 { format!("{}...(truncated)", &text[..500]) } else { text.to_string() };

  if message_type == "Object" {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
      if let Some(event) = value.get("event").and_then(|e| e.as_str()) {
        if event == "systemStatus" || event == "subscriptionStatus" || event == "heartbeat" {
          return;
        }
        println!("DEBUG: {} message format (event={}): {}", message_type, event, truncated);
        return;
      }
    }
  }

  println!("DEBUG: {} message format: {}", message_type, truncated);
}

async fn handle_ws_message(
  text: &str, redb: &AsyncRedb, redis_connection: Option<&mut redis::aio::MultiplexedConnection>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
  match serde_json::from_str::<KrakenMessage>(text) {
    Ok(message) => {
      match message {
        KrakenMessage::TradeMessage(trade_message) => {
          let _channel_id = trade_message.0;
          let trades_data = &trade_message.1;
          let channel_name = &trade_message.2;
          let pair = &trade_message.3;

          if channel_name == "trade" {
            let mut trades = Vec::new();

            for trade_data in trades_data {
              if trade_data.len() < 5 {
                if DEBUG_WS_MESSAGES {
                  println!("WARNING: Skipping trade data with insufficient fields: {:?}", trade_data);
                }
                continue;
              }

              let price = trade_data[0].to_string();
              let volume = trade_data[1].to_string();
              let timestamp = trade_data[2].to_string();
              let side = trade_data[3].to_string();
              let order_type = trade_data[4].to_string();
              let misc = trade_data.get(5).map(|v| v.to_string()).unwrap_or_default();

              let index = redb.run(GetNextTradeCounterRequest).await?;
              let trade_id = format!("{}-{}", index, price);

              let id_exists = redb.run(CheckIdExistsRequest(trade_id.clone())).await?;

              if id_exists {
                println!("WARNING: Duplicate trade detected with ID: {}", trade_id);
                continue;
              }

              redb.run(StoreIdWithIndexRequest { id: trade_id.clone(), index }).await?;

              let trade =
                Trade { id: trade_id.clone(), price, volume, timestamp, side, order_type, misc, pair: pair.clone() };

              println!(
                "TRADE [{}] #{}: {} {} {} price: {}, volume: {}, type: {}",
                trade.id,
                index,
                Local::now().format("%H:%M:%S"),
                pair,
                if trade.side == "b" { "BUY" } else { "SELL" },
                trade.price,
                trade.volume,
                trade.order_type
              );

              trades.push((trade, index));
            }

            if !trades.is_empty() {
              let _count = redb.run(StoreTradeBatchRequest(trades.clone())).await?;

              if let Some(redis_connection) = redis_connection {
                for (trade, _) in &trades {
                  let trade_json = serde_json::to_string(trade)?;

                  let publish_result: RedisResult<()> = redis_connection.publish("trades", trade_json).await;

                  match publish_result {
                    Ok(_) => {}
                    Err(e) => {
                      let error_string = e.to_string();
                      if error_string.contains("connection")
                        || error_string.contains("closed")
                        || error_string.contains("refused")
                        || error_string.contains("broken pipe")
                      {
                        return Err(Box::new(std::io::Error::new(
                          std::io::ErrorKind::ConnectionRefused,
                          format!("Redis connection lost: {}", e),
                        )));
                      } else {
                        return Err(Box::new(e));
                      }
                    }
                  }
                }
              }
            }
          }
        }
        KrakenMessage::EventMessage(event) => match event {
          KrakenEventMessage::SystemStatus(status) => {
            println!("System status: {} (connection ID: {})", status.status, status.connection_id);
          }
          KrakenEventMessage::SubscriptionStatus(subscription) => {
            println!(
              "Subscription to {} for {} is {}",
              subscription.channel_name, subscription.pair, subscription.status
            );
          }
          KrakenEventMessage::Heartbeat => {
            if DEBUG_WS_MESSAGES {
              println!("Received heartbeat");
            }
          }
          KrakenEventMessage::Unknown => {
            log_message_format("Unknown event", text);

            if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
              if let Some(event) = value.get("event").and_then(|e| e.as_str()) {
                println!("Unrecognized event type: {}", event);
                println!("Consider adding this event type to the KrakenEventMessage enum");

                if let Some(obj) = value.as_object() {
                  let keys: Vec<_> = obj.keys().collect();
                  println!("Available fields: {:?}", keys);
                }
              }
            }
          }
        },
        KrakenMessage::Generic(value) => {
          if value.is_array() {
            log_message_format("Array", text);

            let arr = value.as_array().unwrap();

            if arr.len() >= 4 && arr[2].is_string() {
              let channel_name = arr[2].as_str().unwrap();
              println!("Appears to be a channel message for: {}", channel_name);

              if channel_name == "trade" && arr[1].is_array() {
                println!("This appears to be trade data in an unexpected format - investigating...");

                if let Some(first_item) = arr[1].as_array().and_then(|items| items.first()) {
                  println!(
                    "- First item type: {}",
                    if first_item.is_array() {
                      "array"
                    } else if first_item.is_object() {
                      "object"
                    } else {
                      "other"
                    }
                  );
                  println!("- First item: {:?}", first_item);
                }
              }
            }
          } else if value.is_object() {
            log_message_format("Object", text);

            if let Some(event) = value.get("event").and_then(|e| e.as_str()) {
              if event != "systemStatus" && event != "subscriptionStatus" && event != "heartbeat" {
                println!("Unhandled event type: {}", event);
                if let Some(obj) = value.as_object() {
                  let keys: Vec<_> = obj.keys().collect();
                  println!("Available fields: {:?}", keys);
                }
              }
            }
          } else {
            println!("Received message with unexpected format: {}", text);
          }
        }
      }
      Ok(())
    }
    Err(e) => {
      eprintln!("Failed to parse WebSocket message: {}", e);
      log_message_format("Unparseable", text);

      if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        if value.is_array() {
          println!("Message is an array with {} elements", value.as_array().unwrap().len());

          if value.as_array().unwrap().len() >= 3 {
            println!("Possible message format issue. First 3 elements types:");
            for (i, item) in value.as_array().unwrap().iter().take(3).enumerate() {
              let type_name = if item.is_array() {
                "array"
              } else if item.is_object() {
                "object"
              } else if item.is_string() {
                "string"
              } else if item.is_number() {
                "number"
              } else {
                "other"
              };
              println!("Element {}: {}", i, type_name);
            }
          }
        } else if value.is_object() {
          if let Some(event) = value.get("event") {
            println!("Message is an object with 'event' = {}", event);
          } else {
            println!("Message is an object without 'event' field");
          }
        }
      } else {
        println!("Message is not valid JSON!");
      }

      Err(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("Invalid WebSocket message format: {}", e),
      )))
    }
  }
}

async fn get_trades_count(State(state): State<AppState>) -> (StatusCode, String) {
  match state.redb.run(GetTotalTradesCountRequest).await {
    Ok(count) => (StatusCode::OK, count.to_string()),
    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", e)),
  }
}

async fn get_trades(State(state): State<AppState>, Query(params): Query<TradeQueryParams>) -> (StatusCode, String) {
  const MAX_BATCH_SIZE: u64 = 1000;

  let start_index = params.start_index.unwrap_or(1);
  let requested_limit = params.limit.unwrap_or(100);
  let limit = std::cmp::min(requested_limit, MAX_BATCH_SIZE);

  match state.redb.run(GetTradesByRangeRequest { start_index, limit }).await {
    Ok(trades) => match serde_json::to_string(&trades) {
      Ok(json) => (StatusCode::OK, json),
      Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Error serializing response: {}", e)),
    },
    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)),
  }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
  let args = Args::parse();

  fs::create_dir_all(&Path::new("./.db"))?;
  let db = Database::create("./.db/demo.redb")?;

  {
    let write_txn = db.begin_write()?;
    {
      let _ = write_txn.open_table(TRADES)?;
      let _ = write_txn.open_table(TRADE_IDS)?;
      let _ = write_txn.open_table(TRADES_META)?;

      let mut trades_meta = write_txn.open_table(TRADES_META)?;
      if trades_meta.get("total")?.is_none() {
        trades_meta.insert("total", 0u64)?;
      }
    }
    write_txn.commit()?;
  }

  let redb = AsyncRedb::new(db);

  let app_state = AppState { redb };

  let redb_clone = app_state.redb.clone();
  let redis_url = args.redis.clone();

  let api_router = Router::new()
    .route("/trades/count", get(get_trades_count))
    .route("/trades", get(get_trades))
    .with_state(app_state.clone());

  let http_addr = SocketAddr::from(([127, 0, 0, 1], args.port));
  let http_server = tokio::spawn(async move {
    println!("HTTP server listening on {}", http_addr);
    let listener = tokio::net::TcpListener::bind(http_addr).await.expect("Failed to bind HTTP server");
    serve(listener, api_router).await.expect("Failed to start HTTP server");
  });

  let ws_host = args.ws_host.clone();
  let ws_subscribe_cmd = args.ws_subscribe_cmd.clone();
  let ws_task = tokio::spawn(async move {
    loop {
      println!("Connecting to WebSocket at {}", ws_host);
      let url = match Url::parse(&ws_host) {
        Ok(url) => url,
        Err(err) => {
          eprintln!("Failed to parse WebSocket URL: {}", err);
          sleep(Duration::from_secs(5)).await;
          continue;
        }
      };

      let redis_client = match redis::Client::open(redis_url.clone()) {
        Ok(client) => client,
        Err(err) => {
          eprintln!("Failed to create Redis client: {}", err);
          sleep(Duration::from_secs(5)).await;
          continue;
        }
      };

      let mut redis_connection = None;
      match redis_client.get_multiplexed_async_connection().await {
        Ok(conn) => {
          println!("Successfully connected to Redis at {}", redis_url);
          redis_connection = Some(conn);
        }
        Err(err) => {
          eprintln!("Failed to connect to Redis: {} - Will retry after processing messages", err);
        }
      }

      match connect_async(url.to_string()).await {
        Ok((ws_stream, _)) => {
          println!("WebSocket connected successfully");
          let (mut write, mut read) = ws_stream.split();

          match serde_json::from_str::<KrakenSubscribeMessage>(&ws_subscribe_cmd) {
            Ok(subscription_message) => {
              let json_string = match serde_json::to_string(&subscription_message) {
                Ok(json) => json,
                Err(e) => {
                  eprintln!("Failed to serialize subscription message: {}", e);
                  continue;
                }
              };

              if let Err(e) = write.send(Message::Text(json_string.into())).await {
                eprintln!("Failed to send subscription message: {}", e);
                continue;
              }

              let pairs = subscription_message.pair.join(", ");
              println!("Sent subscription request for {} ({}) data", pairs, subscription_message.subscription.name);
            }
            Err(e) => match serde_json::from_str::<serde_json::Value>(&ws_subscribe_cmd) {
              Ok(generic_subscription) => {
                if let Err(e) = write.send(Message::Text(generic_subscription.to_string().into())).await {
                  eprintln!("Failed to send subscription message: {}", e);
                  continue;
                }
                println!("Sent subscription request: {}", generic_subscription);
              }
              Err(_) => {
                eprintln!("Failed to parse subscription command: {}", e);
                sleep(Duration::from_secs(5)).await;
                continue;
              }
            },
          }

          while let Some(message_result) = read.next().await {
            if redis_connection.is_none() {
              match redis_client.get_multiplexed_async_connection().await {
                Ok(conn) => {
                  println!("Successfully reconnected to Redis at {}", redis_url);
                  redis_connection = Some(conn);
                }
                Err(_) => {}
              }
            }

            match message_result {
              Ok(message) => match message {
                Message::Text(text) => match handle_ws_message(&text, &redb_clone, redis_connection.as_mut()).await {
                  Ok(_) => {}
                  Err(e) => {
                    let error_string = e.to_string();
                    if error_string.contains("Redis connection lost")
                      || error_string.contains("redis")
                      || error_string.contains("broken pipe")
                    {
                      eprintln!("Lost connection to Redis: {}", error_string);
                      redis_connection = None;
                      println!("Will continue processing messages and retry Redis connection later");
                    } else {
                      eprintln!("Error handling WebSocket message: {}", e);
                    }
                  }
                },
                Message::Binary(binary) => {
                  println!("Received binary message: {} bytes", binary.len());
                }
                Message::Ping(data) => {
                  if let Err(e) = write.send(Message::Pong(data)).await {
                    eprintln!("Failed to send pong: {}", e);
                    break;
                  }
                }
                Message::Close(_) => {
                  println!("WebSocket closed by server");
                  break;
                }
                _ => {}
              },
              Err(e) => {
                eprintln!("Error receiving message: {}", e);
                break;
              }
            }
          }
        }
        Err(err) => {
          eprintln!("Failed to connect to WebSocket: {}", err);
        }
      }

      println!("Reconnecting in 5 seconds...");
      sleep(Duration::from_secs(5)).await;
    }
  });

  let mut int_signal = signal(SignalKind::interrupt())?;
  let mut term_signal = signal(SignalKind::terminate())?;

  tokio::select! {
    _ = int_signal.recv() => {
      println!("Received SIGINT, shutting down");
    }
    _ = term_signal.recv() => {
      println!("Received SIGTERM, shutting down");
    }
  }

  http_server.abort();
  ws_task.abort();

  match ws_task.await {
    Ok(_) => {}
    Err(e) => {
      if !e.is_cancelled() {
        eprintln!("Error in websocket task: {}", e);
      }
    }
  }

  Ok(())
}
