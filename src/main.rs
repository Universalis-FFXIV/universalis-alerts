use std::io::Cursor;

use bson::Document;
use error_chain::error_chain;
use futures_util::{pin_mut, SinkExt, StreamExt};
use mysql_async::{params, prelude::*, Pool};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

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

#[derive(Serialize, Debug, Clone)]
struct DiscordEmbedFooter<'a> {
    text: &'a str,
    icon_url: &'a str,
}

#[derive(Serialize, Debug, Clone)]
struct DiscordEmbedAuthor<'a> {
    name: &'a str,
    icon_url: &'a str,
}

#[derive(Serialize, Debug, Clone)]
struct DiscordEmbed<'a> {
    url: &'a str,
    title: &'a str,
    description: &'a str,
    color: u32,
    footer: DiscordEmbedFooter<'a>,
    author: DiscordEmbedAuthor<'a>,
}

#[derive(Serialize, Debug)]
struct DiscordWebhookPayload<'a> {
    embeds: Vec<DiscordEmbed<'a>>,
}

#[derive(Deserialize, Debug, Clone)]
struct Item {
    #[serde(rename = "Name")]
    name: String,
}

#[derive(Serialize, Debug, Clone)]
struct SubscribeEvent<'a> {
    event: &'a str,
    channel: &'a str,
}

#[derive(Deserialize, Debug, Clone)]
struct Listing {
    #[serde(rename = "pricePerUnit")]
    unit_price: i32,
    quantity: i32,
    hq: bool,
}

#[derive(Deserialize, Debug, Clone)]
struct ListingsAddEvent {
    #[serde(rename = "item")]
    item_id: i32,
    #[serde(rename = "world")]
    world_id: i32,
    listings: Vec<Listing>,
}

#[derive(Debug)]
struct UserAlert {
    discord_webhook: Option<String>,
    trigger: String,
}

#[derive(Deserialize, Debug, Clone)]
enum TriggerReason {
    PriceLessThan { unit_price: i32 },
}

#[derive(Deserialize, Debug, Clone)]
struct AlertTrigger {
    reasons: Vec<TriggerReason>,
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
    let alerts = r"SELECT `discord_webhook`, `trigger` FROM `users_alerts_next` WHERE `world_id` = :world_id AND `item_id` = :item_id".with(params! {
        "world_id" => world_id,
        "item_id" => item_id,
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
            
            let alerts = get_alerts_for_world_item(ev.world_id, 5, &pool).await.unwrap();
            for (alert, _) in alerts {
                // send webhook message
                let item = get_item(ev.item_id, &client).await.unwrap();
                let market_url = format!("https://universalis.app/market/{}", ev.item_id);
                let discord_webhook = alert.discord_webhook.unwrap();
                let embed_title = format!("Alert triggered for {}", item.name);
                let embed_description = format!("One of your alerts has been triggered for the following reason(s):\n```c\nreasons\n```\nYou can view the item page on Universalis by clicking [this link]({}).", market_url);
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
                let serialized = serde_json::to_string(&payload).unwrap();

                client
                    .post(discord_webhook)
                    .header("Content-Type", "application/json")
                    .body(serialized)
                    .send()
                    .await
                    .unwrap();
            }
        })
    };

    pin_mut!(on_message);
    on_message.await;

    Ok(())
}
