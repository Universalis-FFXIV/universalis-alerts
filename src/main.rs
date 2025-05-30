#[macro_use]
extern crate log;

use std::env;

use crate::discord::*;
use crate::errors::*;
use crate::trigger::*;
use crate::universalis::*;
use crate::xivapi::*;
use dotenv::dotenv;
use futures_util::{pin_mut, SinkExt, StreamExt};
use itertools::Itertools;
use metrics::counter;
use metrics_exporter_prometheus::PrometheusBuilder;
use mysql_async::{params, prelude::*, Pool};
use opentelemetry::global;
use reqwest::Client;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod discord;
mod errors;
mod trigger;
mod universalis;
mod xivapi;

const MIN_TRIGGER_VERSION: i32 = 0;
const MAX_TRIGGER_VERSION: i32 = 0;

#[derive(Debug)]
struct UserAlert {
    user_id: Option<String>,
    name: String,
    discord_webhook: Option<String>,
    trigger: String,
}

#[tracing::instrument(skip(pool))]
async fn get_alerts_for_world_item(
    world_id: i32,
    item_id: i32,
    pool: &Pool,
) -> Result<Vec<(UserAlert, AlertTrigger)>> {
    // TODO: Add caching for this?
    let mut conn = pool.get_conn().await?;
    let alerts = r"SELECT `user_id`, `name`, `discord_webhook`, `trigger` FROM `users_alerts_next` WHERE `world_id` = :world_id AND (`item_id` = :item_id OR `item_id` = -1) AND `trigger_version` >= :min_trigger_version AND `trigger_version` <= :max_trigger_version".with(params! {
        "world_id" => world_id,
        "item_id" => item_id,
        "min_trigger_version" => MIN_TRIGGER_VERSION,
        "max_trigger_version" => MAX_TRIGGER_VERSION,
    })
        .map(&mut conn, |(user_id, name, discord_webhook, trigger)| {
            let alert = UserAlert {
                user_id,
                name,
                discord_webhook,
                trigger,
            };
            let alert_trigger = serde_json::from_str::<AlertTrigger>(&alert.trigger);
            match alert_trigger {
                Ok(at) => Some((alert, at)),
                Err(err) => {
                    error!("{:?}", err);
                    None
                }
            }
        })
        .await?
        .into_iter()
        .filter_map(|t| t)
        .collect_vec();
    Ok(alerts)
}

fn get_universalis_url(item_id: i32, world_name: &str) -> String {
    format!(
        "https://universalis.app/market/{}?server={}",
        item_id, world_name
    )
}

#[tracing::instrument(
    skip(alert, trigger, trigger_result, client),
    fields(
        user_id = alert.user_id.as_ref().unwrap_or(&"".to_string())
    )
)]
async fn send_discord_message(
    item_id: i32,
    world_id: i32,
    alert: &UserAlert,
    trigger: &AlertTrigger,
    trigger_result: f32,
    client: &Client,
) -> Result<()> {
    let discord_webhook = alert.discord_webhook.as_ref();
    if discord_webhook.is_none() {
        return Ok(());
    }
    let discord_webhook = discord_webhook.unwrap();

    let item = get_item(item_id).await?;
    let world = get_world(world_id).await?;
    let market_url = get_universalis_url(item_id, &world.name);
    let embed_title = format!("Alert triggered for {} on {}", item.name, world.name);
    let embed_footer_text = format!("universalis.app | {} | All prices include GST", alert.name);
    let embed_description = format!("One of your alerts has been triggered for the following reason(s):\n```c\n{}\n\nValue: {}```\nYou can view the item page on Universalis by clicking [this link]({}).", trigger, trigger_result, market_url);
    let payload = DiscordWebhookPayload {
        embeds: [DiscordEmbed {
            url: &market_url,
            title: &embed_title,
            description: &embed_description,
            color: 0xBD983A,
            footer: DiscordEmbedFooter {
                text: &embed_footer_text,
                icon_url: "https://universalis.app/favicon.png",
            },
            author: DiscordEmbedAuthor {
                name: "Universalis Alert!",
                icon_url: "https://cdn.discordapp.com/emojis/474543539771015168.png",
            },
        }]
        .to_vec(),
    };
    let serialized = serde_json::to_string(&payload)?;

    client
        .post(discord_webhook)
        .header("Content-Type", "application/json")
        .body(serialized)
        .send()
        .await?;

    Ok(())
}

fn parse_event_from_message(data: &[u8]) -> Result<ListingsAddEvent> {
    let ev: ListingsAddEvent = bson::from_slice(data)?;
    Ok(ev)
}

fn serialize_event(ev: &SubscribeEvent) -> Result<Vec<u8>> {
    let serialized = bson::to_bson(&ev)?;
    let mut v: Vec<u8> = Vec::new();
    serialized
        .clone()
        .as_document()
        .map_or(Err(ErrorKind::NotADocument(serialized).into()), |d| {
            d.to_writer(&mut v)?;
            Ok(v)
        })
}

#[tracing::instrument(skip(message, pool, client))]
async fn process(message: Message, pool: &Pool, client: &Client) -> Result<()> {
    // Parse the message into an event
    let data = message.into_data();
    let ev = parse_event_from_message(&data)?;

    // Fetch all matching alerts from the database
    let alerts = get_alerts_for_world_item(ev.world_id, ev.item_id, &pool)
        .await?
        .into_iter()
        .filter_map(|(alert, trigger)| {
            // Evaluate if all trigger conditions were met
            let trigger_result = trigger.evaluate(&ev.listings);
            match trigger_result {
                Some(tr) => Some((alert, trigger, tr)),
                None => None,
            }
        })
        .collect_vec();
    counter!("universalis_alerts_matched", alerts.len() as u64);

    // Send Discord notifications for each matching trigger
    for (alert, trigger, tr) in alerts {
        let sent =
            send_discord_message(ev.item_id, ev.world_id, &alert, &trigger, tr, &client).await;

        // Log any errors that happened while sending the message
        if let Err(err) = sent {
            error!("{:?}", err);
            counter!("universalis_alerts_delivery_error", 1);
        }
    }

    Ok(())
}

async fn connect_and_process(url: url::Url, pool: &Pool) -> Result<()> {
    info!("Connecting to WebSocket server at {}", url);
    let (ws_stream, _) = connect_async(url).await?;
    info!("WebSocket handshake completed");

    let (mut write, read) = ws_stream.split();

    let event = SubscribeEvent {
        event: "subscribe",
        channel: &env::var("UNIVERSALIS_ALERTS_CHANNEL")
            .chain_err(|| "UNIVERSALIS_ALERTS_CHANNEL not set")?,
    };
    let serialized = serialize_event(&event)?;

    // TODO: Ping the connection so it doesn't die
    write.send(Message::Binary(serialized)).await?;

    let client = reqwest::Client::new();
    let on_message = {
        read.for_each_concurrent(None, |message| async {
            let result = match message {
                Ok(m) => {
                    counter!("universalis_alerts_ws_messages_recieved", 1);
                    process(m, &pool, &client).await
                }
                Err(err) => {
                    counter!("universalis_alerts_ws_errors", 1);
                    Err(ErrorKind::Tungstenite(err).into())
                }
            };
            if let Err(err) = result {
                error!("{:?}", err);
            }
        })
    };

    pin_mut!(on_message);
    on_message.await;

    Err(ErrorKind::ConnectionClosed("the connection was closed".to_owned()).into())
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    // Configure logging; set the log level to info
    // if not specified.
    if let Err(_) = env::var("RUST_LOG") {
        env::set_var("RUST_LOG", "info")
    }
    env_logger::init();

    // Configure metrics
    let metrics_builder = PrometheusBuilder::new();
    metrics_builder
        .install()
        .chain_err(|| "failed to install metrics exporter")?;

    // Configure tracing
    let jaeger_agent_url = env::var("UNIVERSALIS_ALERTS_JAEGER_AGENT")
        .chain_err(|| "UNIVERSALIS_ALERTS_JAEGER_AGENT not set")?;
    global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());
    let tracer = opentelemetry_jaeger::new_pipeline()
        .with_agent_endpoint(jaeger_agent_url)
        .with_service_name("universalis_alerts")
        .install_simple()
        .chain_err(|| "failed to install span processor")?;
    let opentelemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    tracing_subscriber::registry()
        .with(opentelemetry)
        .try_init()
        .chain_err(|| "failed to install tracing subscriber")?;

    let database_url =
        env::var("UNIVERSALIS_ALERTS_DB").chain_err(|| "UNIVERSALIS_ALERTS_DB not set")?;
    let pool = Pool::new(database_url.as_str());

    let connect_addr =
        env::var("UNIVERSALIS_ALERTS_WS").chain_err(|| "UNIVERSALIS_ALERTS_WS not set")?;
    let url = url::Url::parse(&connect_addr).chain_err(|| "failed to parse server address")?;

    while let Err(err) = connect_and_process(url.clone(), &pool).await {
        counter!("universalis_alerts_ws_closes", 1);
        error!("{:?}", err)
    }

    Ok(())
}
