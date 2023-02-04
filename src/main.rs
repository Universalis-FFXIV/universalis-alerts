use std::env;
use std::io::Cursor;

use crate::discord::*;
use crate::errors::*;
use crate::trigger::*;
use crate::universalis::*;
use crate::xivapi::*;
use bson::Document;
use dotenv::dotenv;
use futures_util::{pin_mut, SinkExt, StreamExt};
use itertools::Itertools;
use mysql_async::{params, prelude::*, Pool};
use reqwest::Client;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

mod discord;
mod errors;
mod trigger;
mod universalis;
mod xivapi;

const MIN_TRIGGER_VERSION: i32 = 0;
const MAX_TRIGGER_VERSION: i32 = 0;

#[derive(Debug)]
struct UserAlert {
    name: String,
    discord_webhook: Option<String>,
    trigger: String,
}

async fn get_alerts_for_world_item(
    world_id: i32,
    item_id: i32,
    pool: &Pool,
) -> Result<Vec<(UserAlert, AlertTrigger)>> {
    // TODO: Add caching for this?
    let mut conn = pool.get_conn().await?;
    let alerts = r"SELECT `name`, `discord_webhook`, `trigger` FROM `users_alerts_next` WHERE `world_id` = :world_id AND (`item_id` = :item_id OR `item_id` = -1) AND `trigger_version` >= :min_trigger_version AND `trigger_version` <= :max_trigger_version".with(params! {
        "world_id" => world_id,
        "item_id" => item_id,
        "min_trigger_version" => MIN_TRIGGER_VERSION,
        "max_trigger_version" => MAX_TRIGGER_VERSION,
    })
        .map(&mut conn, |(name, discord_webhook, trigger)| {
            let alert = UserAlert {
                name,
                discord_webhook,
                trigger,
            };
            let alert_trigger = serde_json::from_str::<AlertTrigger>(&alert.trigger);
            match alert_trigger {
                Ok(at) => Some((alert, at)),
                // TODO: Log this error
                Err(_) => None
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
    let mut reader = Cursor::new(data.clone());
    let document = Document::from_reader(&mut reader)?;
    let ev: ListingsAddEvent = bson::from_bson(document.into())?;
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

async fn process(message: Message, pool: &Pool, client: &Client) -> Result<()> {
    // Parse the message into an event
    let data = message.into_data();
    let ev = parse_event_from_message(&data)?;

    // Fetch all matching alerts from the database
    let alerts = get_alerts_for_world_item(ev.world_id, ev.item_id, &pool).await?;
    for (alert, trigger) in alerts {
        // Send webhook message if all trigger conditions are met
        let sent = trigger
            .evaluate(&ev.listings)
            .map(|tr| send_discord_message(ev.item_id, ev.world_id, &alert, &trigger, tr, &client));

        // Log any errors that happened while sending the message
        if let Some(s) = sent {
            if let Err(err) = s.await {
                println!("{:?}", err);
            }
        }
    }

    Ok(())
}

async fn connect_and_process(url: url::Url, pool: &Pool) -> Result<()> {
    let (ws_stream, _) = connect_async(url).await?;
    println!("WebSocket handshake has been successfully completed");

    let (mut write, read) = ws_stream.split();

    let event = SubscribeEvent {
        event: "subscribe",
        channel: &env::var("UNIVERSALIS_ALERTS_CHANNEL")?,
    };
    let serialized = serialize_event(&event)?;

    // TODO: Ping the connection so it doesn't die
    write.send(Message::Binary(serialized)).await?;

    let client = reqwest::Client::new();
    let on_message = {
        read.for_each_concurrent(None, |message| async {
            let result = match message {
                Ok(m) => process(m, &pool, &client).await,
                Err(err) => Err(ErrorKind::Tungstenite(err).into()),
            };
            if let Err(err) = result {
                println!("{:?}", err);
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

    // TODO: Enable tokio tracing
    // TODO: Add metrics
    // TODO: Add logging
    // TODO: Log failures instead of just yeeting errors

    let database_url = env::var("UNIVERSALIS_ALERTS_DB")?;
    let pool = Pool::new(database_url.as_str());

    let connect_addr = env::var("UNIVERSALIS_ALERTS_WS")?;
    let url = url::Url::parse(&connect_addr)?;

    while let Err(err) = connect_and_process(url.clone(), &pool).await {
        println!("{:?}", err)
    }

    Ok(())
}
