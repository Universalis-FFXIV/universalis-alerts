use error_chain::error_chain;

error_chain! {
    foreign_links {
        Io(std::io::Error);
        HttpRequest(reqwest::Error);
        Url(url::ParseError);
        Tungstenite(tungstenite::Error);
        BsonDe(bson::de::Error);
        BsonSer(bson::ser::Error);
        Json(serde_json::Error);
        Database(mysql_async::Error);
        Env(std::env::VarError);
    }

    errors {
        NotADocument(b: bson::Bson) {
            description("not a document"),
            display("not a document: {}", b),
        }
    }
}
