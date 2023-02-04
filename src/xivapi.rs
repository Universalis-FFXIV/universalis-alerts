use crate::errors::*;
use reqwest::Client;
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

pub async fn get_item(id: i32, client: &Client) -> Result<Item> {
    let url = format!("https://xivapi.com/Item/{}?columns=Name", id);
    let res = client.get(url).send().await?;
    let response_text = res.text().await?;
    let item = serde_json::from_str(&response_text)?;
    Ok(item)
}

pub async fn get_world(id: i32, client: &Client) -> Result<World> {
    let url = format!("https://xivapi.com/World/{}?columns=Name", id);
    let res = client.get(url).send().await?;
    let response_text = res.text().await?;
    let item = serde_json::from_str(&response_text)?;
    Ok(item)
}
