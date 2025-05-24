use sqlx::PgPool;
use std::error::Error;
use std::path::PathBuf;
use std::fs;
use log::info;
use anyhow::{Context, Result};

// Hàm chạy migrations SQL
pub async fn run_migrations(pool: &PgPool) -> Result<(), Box<dyn Error>> {
    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/migrations");
    
    info!("Reading migration files from: {}", migrations_dir.display());

    // Đọc các file SQL trong thư mục migrations
    let mut migration_files = Vec::new();
    for entry_result in fs::read_dir(&migrations_dir).context(format!("Failed to read migrations directory: {}", migrations_dir.display()))? {
        let entry = entry_result.context("Failed to read directory entry")?;
        let path = entry.path();
        
        if path.is_file() && path.extension().map_or(false, |ext| ext == "sql") {
            info!("Found migration file: {}", path.display());
            migration_files.push(path);
        }
    }
    
    if migration_files.is_empty() {
        info!("No migration files found in {}", migrations_dir.display());
    } else {
        info!("Found {} migration files", migration_files.len());
    }
    
    // Sắp xếp theo tên để đảm bảo thứ tự chạy đúng
    migration_files.sort();
    
    // Chạy từng file migration
    for file_path in migration_files {
        let sql = fs::read_to_string(&file_path)
            .context(format!("Failed to read migration file: {}", file_path.display()))?;
        
        let file_name = file_path.file_name()
            .and_then(|f| f.to_str())
            .unwrap_or_else(|| {
                log::warn!("Could not get file name from path: {}, using 'unknown'", file_path.display());
                "unknown"
            });
        
        info!("Running migration: {}", file_name);
        
        sqlx::query(&sql)
            .execute(pool)
            .await
            .context(format!("Failed to execute migration: {}", file_name))?;
        
        info!("Successfully applied migration: {}", file_name);
    }
    
    Ok(())
}

// Hàm kiểm tra status của migrations
pub async fn migration_status(pool: &PgPool) -> Result<Vec<String>, Box<dyn Error>> {
    // Tạo bảng migration_history nếu chưa tồn tại
    info!("Creating migration_history table if it doesn't exist");
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS migration_history (
            id SERIAL PRIMARY KEY,
            migration_name VARCHAR(255) NOT NULL UNIQUE,
            applied_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
        )"
    )
    .execute(pool)
    .await
    .context("Failed to create migration_history table")?;
    
    // Lấy danh sách các migration đã áp dụng
    info!("Fetching applied migrations");
    let applied_migrations: Vec<String> = sqlx::query_scalar(
        "SELECT migration_name FROM migration_history ORDER BY applied_at"
    )
    .fetch_all(pool)
    .await
    .context("Failed to fetch applied migrations")?;
    
    info!("Found {} applied migrations", applied_migrations.len());
    Ok(applied_migrations)
} 