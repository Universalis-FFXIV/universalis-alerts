use std::io::Cursor;

use crate::discord::*;
use crate::trigger::*;
use crate::universalis::*;
use bson::Document;
use error_chain::error_chain;
use futures_util::{pin_mut, SinkExt, StreamExt};
use mysql_async::{params, prelude::*, Pool};
use reqwest::Client;
use serde::Deserialize;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

mod discord;
mod trigger;
mod universalis;

error_chain! {
    foreign_links {
        Io(std::io::Error);
        HttpRequest(reqwest::Error);
        Url(url::ParseError);
        Tungstenite(tungstenite::Error);
        Bson(bson::ser::Error);
        Json(serde_json::Error);
        Database(mysql_async::Error);
    }
}

const MIN_TRIGGER_VERSION: i32 = 0;
const MAX_TRIGGER_VERSION: i32 = 0;

#[derive(Deserialize, Debug, Clone)]
struct Item {
    #[serde(rename = "Name")]
    name: String,
}

#[derive(Debug)]
struct UserAlert {
    discord_webhook: Option<String>,
    trigger: String,
}

async fn get_item(id: i32, client: &Client) -> Result<Item> {
    let url = format!("https://xivapi.com/Item/{}?columns=Name", id);
    let res = client.get(url).send().await?;
    let response_text = res.text().await?;
    let item = serde_json::from_str(&response_text)?;
    Ok(item)
}

async fn get_alerts_for_world_item(
    world_id: i32,
    item_id: i32,
    pool: &Pool,
) -> Result<Vec<(UserAlert, AlertTrigger)>> {
    let mut conn = pool.get_conn().await?;
    let alerts = r"SELECT `discord_webhook`, `trigger` FROM `users_alerts_next` WHERE `world_id` = :world_id AND `item_id` = :item_id AND `trigger_version` >= :min_trigger_version AND `trigger_version` <= :max_trigger_version".with(params! {
        "world_id" => world_id,
        "item_id" => item_id,
        "min_trigger_version" => MIN_TRIGGER_VERSION,
        "max_trigger_version" => MAX_TRIGGER_VERSION,
    })
        .map(&mut conn, |(discord_webhook, trigger)| {
            let alert = UserAlert {
                discord_webhook,
                trigger,
            };
            let alert_trigger: AlertTrigger = serde_json::from_str(&alert.trigger).unwrap();
            (alert, alert_trigger)
        })
        .await?;
    Ok(alerts)
}

fn get_universalis_url(item_id: i32) -> String {
    format!("https://universalis.app/market/{}", item_id)
}

async fn send_discord_message(
    item_id: i32,
    alert: &UserAlert,
    trigger: &AlertTrigger,
    trigger_result: f32,
    client: &Client,
) -> Result<()> {
    let item = get_item(item_id, &client).await?;
    let market_url = get_universalis_url(item_id);
    let discord_webhook = alert.discord_webhook.as_ref().unwrap();
    let embed_title = format!("Alert triggered for {}", item.name);
    let embed_description = format!("One of your alerts has been triggered for the following reason(s):\n```c\n{}\n\nValue: {}```\nYou can view the item page on Universalis by clicking [this link]({}).", trigger, trigger_result, market_url);
    let payload = DiscordWebhookPayload {
        embeds: [DiscordEmbed {
            url: &market_url,
            title: &embed_title,
            description: &embed_description,
            color: 0xBD983A,
            footer: DiscordEmbedFooter {
                text: "universalis.app",
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

#[tokio::main]
async fn main() -> Result<()> {
    let database_url = "mysql://dalamud:dalamud@localhost:4003/dalamud";
    let pool = Pool::new(database_url);

    let connect_addr = "wss://universalis.app/api/ws";
    let url = url::Url::parse(&connect_addr)?;

    let (ws_stream, _) = connect_async(url).await?;
    println!("WebSocket handshake has been successfully completed");

    let (mut write, read) = ws_stream.split();

    let event = SubscribeEvent {
        event: "subscribe",
        channel: "listings/add{world=74}",
    };
    let serialized = bson::to_bson(&event)?;
    let mut v: Vec<u8> = Vec::new();
    serialized.as_document().unwrap().to_writer(&mut v)?;

    write.send(Message::Binary(v)).await?;

    let client = reqwest::Client::new();
    let on_message = {
        read.for_each(|message| async {
            let data = message.unwrap().into_data();
            let mut reader = Cursor::new(data.clone());
            let document = Document::from_reader(&mut reader).unwrap();
            let ev: ListingsAddEvent = bson::from_bson(document.into()).unwrap();

            let alerts = get_alerts_for_world_item(ev.world_id, 5, &pool)
                .await
                .unwrap();
            for (alert, trigger) in alerts {
                // Check if all trigger conditions are met
                let trigger_result = trigger.evaluate(&ev.listings);
                if trigger_result.is_none() {
                    continue;
                }

                // send webhook message
                send_discord_message(
                    ev.item_id,
                    &alert,
                    &trigger,
                    trigger_result.unwrap(),
                    &client,
                )
                .await
                .unwrap();
            }
        })
    };

    pin_mut!(on_message);
    on_message.await;

    Ok(())
}
