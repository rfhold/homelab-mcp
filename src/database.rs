use sqlx::{PgPool, migrate::MigrateError};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Runs only the application-owned migrations embedded in this crate.
pub async fn migrate(pool: &PgPool) -> Result<(), MigrateError> {
    MIGRATOR.run(pool).await
}
