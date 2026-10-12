//! Server binary for AI Provider Connector.

#[tokio::main]
async fn main() {
    println!("ai-provider-connector-server starting...");
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_server_smoke() {
        assert_eq!(2 + 2, 4);
    }
}
