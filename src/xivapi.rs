use crate::errors::*;
use cached::proc_macro::cached;
use serde::Deserialize;

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
    let url = format!("https://xivapi.com/Item/{}?columns=Name", id);
    let client = reqwest::Client::new();

    let res = client.get(url).send().await?;
    let response_text = res.text().await?;
    let item = serde_json::from_str(&response_text)?;

    Ok(item)
}

#[cached(size = 500, time = 60, result = true)]
pub async fn get_world(id: i32) -> Result<World> {
    let url = format!("https://xivapi.com/World/{}?columns=Name", id);
    let client = reqwest::Client::new();

    let res = client.get(url).send().await?;
    let response_text = res.text().await?;
    let world = serde_json::from_str(&response_text)?;

    Ok(world)
}
