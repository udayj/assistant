mod errors;
mod services;
mod types;
pub use errors::DatabaseError;
pub use services::DatabaseService;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;
    use dotenvy::dotenv;

    #[tokio::test]
    async fn test_database_connection() {
        dotenv().ok();
        let db = DatabaseService::new().expect("Failed to create database service");

        // Test user lookup (should return None for non-existent user)
        let result = db.get_user_by_phone("+999999999999").await;
        println!("result:{:#?}", result);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        println!("✅ Database connection test passed");
    }
}
