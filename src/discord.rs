use serde::Serialize;

#[derive(Serialize, Debug, Clone)]
pub struct DiscordEmbedFooter<'a> {
    pub text: &'a str,
    pub icon_url: &'a str,
}

#[derive(Serialize, Debug, Clone)]
pub struct DiscordEmbedAuthor<'a> {
    pub name: &'a str,
    pub icon_url: &'a str,
}

#[derive(Serialize, Debug, Clone)]
pub struct DiscordEmbed<'a> {
    pub url: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub color: u32,
    pub footer: DiscordEmbedFooter<'a>,
    pub author: DiscordEmbedAuthor<'a>,
}

#[derive(Serialize, Debug)]
pub struct DiscordWebhookPayload<'a> {
    pub embeds: Vec<DiscordEmbed<'a>>,
}
