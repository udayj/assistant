use super::errors::DatabaseError;
use postgrest::Postgrest;
use std::env;

mod cost;
mod session;
mod user;
pub struct DatabaseService {
    pub client: Postgrest,
}

impl DatabaseService {
    pub fn new() -> Result<Self, DatabaseError> {
        let url = env::var("SUPABASE_URL")
            .map_err(|_| DatabaseError::ConnectionError("SUPABASE_URL not found".to_string()))?;
        let service_key = env::var("SUPABASE_KEY")
            .map_err(|_| DatabaseError::ConnectionError("SUPABASE_KEY not found".to_string()))?;

        let rest_url = format!("{}/rest/v1", url);
        let client = Postgrest::new(&rest_url)
            .insert_header("apikey", &service_key)
            .insert_header("Authorization", &format!("Bearer {}", service_key));

        Ok(Self { client })
    }
}
