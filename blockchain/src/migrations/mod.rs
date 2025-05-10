use sqlx::PgPool;
use std::error::Error;
use std::path::PathBuf;
use std::fs;

// Hàm chạy migrations SQL
pub async fn run_migrations(pool: &PgPool) -> Result<(), Box<dyn Error>> {
    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/migrations");
    
    // Đọc các file SQL trong thư mục migrations
    let mut migration_files = Vec::new();
    for entry in fs::read_dir(migrations_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_file() && path.extension().map_or(false, |ext| ext == "sql") {
            migration_files.push(path);
        }
    }
    
    // Sắp xếp theo tên để đảm bảo thứ tự chạy đúng
    migration_files.sort();
    
    // Chạy từng file migration
    for file_path in migration_files {
        let sql = fs::read_to_string(&file_path)?;
        let file_name = file_path.file_name().unwrap().to_string_lossy();
        
        log::info!("Running migration: {}", file_name);
        
        sqlx::query(&sql)
            .execute(pool)
            .await?;
        
        log::info!("Successfully applied migration: {}", file_name);
    }
    
    Ok(())
}

// Hàm kiểm tra status của migrations
pub async fn migration_status(pool: &PgPool) -> Result<Vec<String>, Box<dyn Error>> {
    // Tạo bảng migration_history nếu chưa tồn tại
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS migration_history (
            id SERIAL PRIMARY KEY,
            migration_name VARCHAR(255) NOT NULL UNIQUE,
            applied_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
        )"
    )
    .execute(pool)
    .await?;
    
    // Lấy danh sách các migration đã áp dụng
    let applied_migrations: Vec<String> = sqlx::query_scalar(
        "SELECT migration_name FROM migration_history ORDER BY applied_at"
    )
    .fetch_all(pool)
    .await?;
    
    Ok(applied_migrations)
} 