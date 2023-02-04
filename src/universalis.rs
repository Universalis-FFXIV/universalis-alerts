use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug, Clone)]
pub struct SubscribeEvent<'a> {
    pub event: &'a str,
    pub channel: &'a str,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Listing {
    #[serde(rename = "pricePerUnit")]
    pub unit_price: i32,
    pub quantity: i32,
    pub hq: bool,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ListingsAddEvent {
    #[serde(rename = "item")]
    pub item_id: i32,
    #[serde(rename = "world")]
    pub world_id: i32,
    pub listings: Vec<Listing>,
}
