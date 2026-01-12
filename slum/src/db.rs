//! Database layer for slum fleet management
//!
//! Stores server and tenant information in SQLite.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Row, Sqlite};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use tracing::info;

/// SQLite connection pool
pub type DbPool = Pool<Sqlite>;

/// A server in the fleet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub url: String,
    pub region: Option<String>,
    pub status: ServerStatus,
    pub last_seen: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Server status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerStatus {
    Online,
    Offline,
    Degraded,
    Unknown,
}

impl std::fmt::Display for ServerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerStatus::Online => write!(f, "online"),
            ServerStatus::Offline => write!(f, "offline"),
            ServerStatus::Degraded => write!(f, "degraded"),
            ServerStatus::Unknown => write!(f, "unknown"),
        }
    }
}

impl FromStr for ServerStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "online" => Ok(ServerStatus::Online),
            "offline" => Ok(ServerStatus::Offline),
            "degraded" => Ok(ServerStatus::Degraded),
            _ => Ok(ServerStatus::Unknown),
        }
    }
}

/// A tenant (customer/app) that can be routed to servers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub server_id: String,
    pub process: String,
    pub instance_id: String,
    pub created_at: DateTime<Utc>,
}

/// Database for fleet management
pub struct SlumDb {
    pool: DbPool,
}

impl SlumDb {
    /// Initialize the database
    pub async fn init(path: &Path) -> Result<Self> {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let options = SqliteConnectOptions::from_str(&format!("sqlite:{}?mode=rwc", path.display()))?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .context("Failed to connect to SQLite database")?;

        // Enable foreign key constraints
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .context("Failed to enable foreign keys")?;

        // Create tables
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS servers (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                url TEXT NOT NULL,
                region TEXT,
                status TEXT NOT NULL DEFAULT 'unknown',
                last_seen TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS tenants (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                domain TEXT NOT NULL UNIQUE,
                server_id TEXT NOT NULL,
                process TEXT NOT NULL,
                instance_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (server_id) REFERENCES servers(id)
            );

            CREATE INDEX IF NOT EXISTS idx_tenants_domain ON tenants(domain);
            CREATE INDEX IF NOT EXISTS idx_tenants_server ON tenants(server_id);
            "#,
        )
        .execute(&pool)
        .await
        .context("Failed to create tables")?;

        info!("Slum database initialized at {:?}", path);
        Ok(Self { pool })
    }

    // --- Server CRUD ---

    /// Add a new server
    pub async fn add_server(&self, server: &Server) -> Result<()> {
        sqlx::query(
            "INSERT INTO servers (id, name, url, region, status, last_seen, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&server.id)
        .bind(&server.name)
        .bind(&server.url)
        .bind(&server.region)
        .bind(server.status.to_string())
        .bind(server.last_seen.map(|dt| dt.to_rfc3339()))
        .bind(server.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get a server by ID
    pub async fn get_server(&self, id: &str) -> Result<Option<Server>> {
        let row = sqlx::query("SELECT * FROM servers WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| Self::row_to_server(&r)))
    }

    /// List all servers
    pub async fn list_servers(&self) -> Result<Vec<Server>> {
        let rows = sqlx::query("SELECT * FROM servers ORDER BY name")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.iter().map(Self::row_to_server).collect())
    }

    /// Update server status
    pub async fn update_server_status(&self, id: &str, status: ServerStatus) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query("UPDATE servers SET status = ?, last_seen = ? WHERE id = ?")
            .bind(status.to_string())
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Delete a server
    pub async fn delete_server(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM servers WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    fn row_to_server(row: &sqlx::sqlite::SqliteRow) -> Server {
        Server {
            id: row.get("id"),
            name: row.get("name"),
            url: row.get("url"),
            region: row.get("region"),
            status: row
                .get::<String, _>("status")
                .parse()
                .unwrap_or(ServerStatus::Unknown),
            last_seen: row
                .get::<Option<String>, _>("last_seen")
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            created_at: row
                .get::<String, _>("created_at")
                .parse::<DateTime<chrono::FixedOffset>>()
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        }
    }

    // --- Tenant CRUD ---

    /// Add a new tenant
    pub async fn add_tenant(&self, tenant: &Tenant) -> Result<()> {
        sqlx::query(
            "INSERT INTO tenants (id, name, domain, server_id, process, instance_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&tenant.id)
        .bind(&tenant.name)
        .bind(&tenant.domain)
        .bind(&tenant.server_id)
        .bind(&tenant.process)
        .bind(&tenant.instance_id)
        .bind(tenant.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get a tenant by ID
    pub async fn get_tenant(&self, id: &str) -> Result<Option<Tenant>> {
        let row = sqlx::query("SELECT * FROM tenants WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| Self::row_to_tenant(&r)))
    }

    /// Get a tenant by domain
    pub async fn get_tenant_by_domain(&self, domain: &str) -> Result<Option<Tenant>> {
        let row = sqlx::query("SELECT * FROM tenants WHERE domain = ?")
            .bind(domain)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| Self::row_to_tenant(&r)))
    }

    /// List all tenants
    pub async fn list_tenants(&self) -> Result<Vec<Tenant>> {
        let rows = sqlx::query("SELECT * FROM tenants ORDER BY name")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.iter().map(Self::row_to_tenant).collect())
    }

    /// List tenants for a server
    pub async fn list_tenants_by_server(&self, server_id: &str) -> Result<Vec<Tenant>> {
        let rows = sqlx::query("SELECT * FROM tenants WHERE server_id = ? ORDER BY name")
            .bind(server_id)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.iter().map(Self::row_to_tenant).collect())
    }

    /// Delete a tenant
    pub async fn delete_tenant(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM tenants WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    fn row_to_tenant(row: &sqlx::sqlite::SqliteRow) -> Tenant {
        Tenant {
            id: row.get("id"),
            name: row.get("name"),
            domain: row.get("domain"),
            server_id: row.get("server_id"),
            process: row.get("process"),
            instance_id: row.get("instance_id"),
            created_at: row
                .get::<String, _>("created_at")
                .parse::<DateTime<chrono::FixedOffset>>()
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        }
    }

    /// Route a domain to its tenant and server
    pub async fn route(&self, domain: &str) -> Result<Option<(Tenant, Server)>> {
        let tenant = match self.get_tenant_by_domain(domain).await? {
            Some(t) => t,
            None => return Ok(None),
        };

        let server = match self.get_server(&tenant.server_id).await? {
            Some(s) => s,
            None => return Ok(None),
        };

        Ok(Some((tenant, server)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_db() -> (SlumDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let db = SlumDb::init(&path).await.unwrap();
        (db, dir)
    }

    fn test_server(id: &str) -> Server {
        Server {
            id: id.to_string(),
            name: format!("Server {}", id),
            url: format!("http://server-{}.example.com", id),
            region: Some("us-east-1".to_string()),
            status: ServerStatus::Online,
            last_seen: Some(Utc::now()),
            created_at: Utc::now(),
        }
    }

    fn test_tenant(id: &str, server_id: &str) -> Tenant {
        Tenant {
            id: id.to_string(),
            name: format!("Tenant {}", id),
            domain: format!("{}.app.example.com", id),
            server_id: server_id.to_string(),
            process: "api".to_string(),
            instance_id: "prod".to_string(),
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_server_crud() {
        let (db, _dir) = create_test_db().await;

        // Create
        let server = test_server("srv1");
        db.add_server(&server).await.unwrap();

        // Read
        let fetched = db.get_server("srv1").await.unwrap().unwrap();
        assert_eq!(fetched.id, "srv1");
        assert_eq!(fetched.name, "Server srv1");
        assert_eq!(fetched.status, ServerStatus::Online);

        // List
        let servers = db.list_servers().await.unwrap();
        assert_eq!(servers.len(), 1);

        // Update status
        db.update_server_status("srv1", ServerStatus::Degraded)
            .await
            .unwrap();
        let updated = db.get_server("srv1").await.unwrap().unwrap();
        assert_eq!(updated.status, ServerStatus::Degraded);

        // Delete
        assert!(db.delete_server("srv1").await.unwrap());
        assert!(db.get_server("srv1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_tenant_crud() {
        let (db, _dir) = create_test_db().await;

        // Add server first (foreign key)
        db.add_server(&test_server("srv1")).await.unwrap();

        // Create tenant
        let tenant = test_tenant("tenant1", "srv1");
        db.add_tenant(&tenant).await.unwrap();

        // Read by ID
        let fetched = db.get_tenant("tenant1").await.unwrap().unwrap();
        assert_eq!(fetched.id, "tenant1");
        assert_eq!(fetched.server_id, "srv1");

        // Read by domain
        let by_domain = db
            .get_tenant_by_domain("tenant1.app.example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(by_domain.id, "tenant1");

        // List
        let tenants = db.list_tenants().await.unwrap();
        assert_eq!(tenants.len(), 1);

        // List by server
        let server_tenants = db.list_tenants_by_server("srv1").await.unwrap();
        assert_eq!(server_tenants.len(), 1);

        // Delete
        assert!(db.delete_tenant("tenant1").await.unwrap());
        assert!(db.get_tenant("tenant1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_route() {
        let (db, _dir) = create_test_db().await;

        // Setup
        db.add_server(&test_server("srv1")).await.unwrap();
        db.add_tenant(&test_tenant("tenant1", "srv1")).await.unwrap();

        // Route existing domain
        let result = db.route("tenant1.app.example.com").await.unwrap();
        assert!(result.is_some());
        let (tenant, server) = result.unwrap();
        assert_eq!(tenant.id, "tenant1");
        assert_eq!(server.id, "srv1");

        // Route non-existent domain
        let not_found = db.route("unknown.example.com").await.unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_foreign_key_constraint() {
        let (db, _dir) = create_test_db().await;

        // Try to add tenant with non-existent server_id - should fail
        let tenant = test_tenant("tenant1", "nonexistent_server");
        let result = db.add_tenant(&tenant).await;
        assert!(result.is_err(), "Should fail due to FK constraint");
    }

    #[test]
    fn test_server_status_display() {
        assert_eq!(ServerStatus::Online.to_string(), "online");
        assert_eq!(ServerStatus::Offline.to_string(), "offline");
        assert_eq!(ServerStatus::Degraded.to_string(), "degraded");
        assert_eq!(ServerStatus::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_server_status_from_str() {
        assert_eq!("online".parse::<ServerStatus>().unwrap(), ServerStatus::Online);
        assert_eq!("offline".parse::<ServerStatus>().unwrap(), ServerStatus::Offline);
        assert_eq!("degraded".parse::<ServerStatus>().unwrap(), ServerStatus::Degraded);
        assert_eq!("invalid".parse::<ServerStatus>().unwrap(), ServerStatus::Unknown);
    }
}
