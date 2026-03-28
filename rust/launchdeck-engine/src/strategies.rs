#![allow(non_snake_case, dead_code)]

use serde_json::{Value, json};

pub fn strategy_registry() -> Value {
    json!({
        "none": {
            "id": "none",
            "label": "None",
            "description": "No post-launch automation."
        },
        "dev-buy": {
            "id": "dev-buy",
            "label": "Dev Buy",
            "description": "Include the configured developer buy during launch where supported."
        },
        "snipe-own-launch": {
            "id": "snipe-own-launch",
            "label": "Snipe Own Launch",
            "description": "Submit separate follow-up buy transactions around 1-2 blocks after launch."
        },
        "automatic-dev-sell": {
            "id": "automatic-dev-sell",
            "label": "Automatic Dev Sell",
            "description": "Sell a configured share of the dev wallet after launch with a short delay."
        }
    })
}
