use crate::errors::*;
use cached::proc_macro::cached;
use metrics::counter;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct Row<T> {
    pub fields: T,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Item {
    #[serde(rename = "Name")]
    pub name: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct World {
    #[serde(rename = "Name")]
    pub name: String,
}

// Unfortunately, it's not possible to reuse the client here,
// since the function arguments are being used as a cache key.

#[cached(size = 500, time = 60, result = true)]
pub async fn get_item(id: i32) -> Result<Item> {
    let url = format!(
        "https://v2.xivapi.com/api/sheet/Item/{}?language=en&fields=Name",
        id
    );
    let client = reqwest::Client::new();

    let res = client.get(url).send().await?;
    let response_text = res.text().await?;
    let row: Row<Item> = serde_json::from_str(&response_text)?;

    counter!("universalis_alerts_xivapi_requests", 1);

    Ok(row.fields)
}

#[cached(size = 500, time = 60, result = true)]
pub async fn get_world(id: i32) -> Result<World> {
    let url = format!(
        "https://v2.xivapi.com/api/sheet/World/{}?language=en&fields=Name",
        id
    );
    let client = reqwest::Client::new();

    let res = client.get(url).send().await?;
    let response_text = res.text().await?;
    let row: Row<World> = serde_json::from_str(&response_text)?;

    counter!("universalis_alerts_xivapi_requests", 1);

    Ok(row.fields)
}
