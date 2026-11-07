//! Values of the pinned build. Moving the pin changes these together.
//! Sources: `design/snapshot/BUILD`, `WEB_BUILD.json`, `TRAFFIC/identify.json`.

pub const CLIENT_BUILD_NUMBER: u32 = 667580;
pub const CLIENT_VERSION: &str = "1.0.9267";
pub const RELEASE_CHANNEL: &str = "stable";
/// `VERSION_HASH` in `WEB_BUILD.json`.
pub const VERSION_HASH: &str = "7f6f109b760a6a8abddc126518351081780500c5";
pub const ELECTRON_VERSION: &str = "44.10.0";
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) discord/1.0.9267 Chrome/152.0.7977.169 Electron/44.10.0 Safari/537.36";
pub const SEC_CH_UA: &str = "\"Not?A_Brand\";v=\"24\", \"Chromium\";v=\"152\"";

pub const API_VERSION: u32 = 9;
pub const WEBAPP_ORIGIN: &str = "https://discordapp.com";
pub const API_BASE: &str = "https://discordapp.com/api/v9";
pub const CDN_HOST: &str = "cdn.discordapp.com";
pub const GATEWAY_ENDPOINT: &str = "wss://gateway.discord.gg";

/// Module 868, `capabilities`: 1734653, or this value with channel obfuscation on, as the pinned session sent.
pub const GATEWAY_CAPABILITIES: u64 = 1767421;
/// Module 903063, `client_heartbeat_version`.
pub const QOS_HEARTBEAT_VERSION: u64 = 31;

/// Discord's epoch, the zero of every snowflake.
pub const SNOWFLAKE_EPOCH_MS: u64 = 1_420_070_400_000;

/// A message nonce: a snowflake for the current moment, as the client generates them.
pub fn nonce_at(unix_ms: u64) -> String {
    ((unix_ms - SNOWFLAKE_EPOCH_MS) << 22).to_string()
}

#[cfg(test)]
mod tests {
    use super::nonce_at;

    #[test]
    fn nonce_is_a_snowflake_for_that_moment() {
        // Recorded send: nonce 1568723187389169664 decodes to this millisecond.
        let recorded: u64 = 1568723187389169664;
        let unix_ms = (recorded >> 22) + super::SNOWFLAKE_EPOCH_MS;
        assert_eq!(nonce_at(unix_ms), (recorded >> 22 << 22).to_string());
    }
}
