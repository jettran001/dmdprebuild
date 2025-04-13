use std::env;
use std::fs::File;
use std::io::{Read, Write};
use walkdir::WalkDir;
use regex::Regex;
use serde_json::{json, Value};

fn main() -> std::io::Result<()> {
    // Lấy thư mục hiện tại
    let current_dir = env::current_dir()?;
    let output_file = r"T:\DiamondChain_preBuild\backup_original\backup\output.json";
    let mut files_content: Vec<Value> = Vec::new();

    // Quét thư mục đệ quy, loại trừ "target"
    for entry in WalkDir::new(current_dir)
        .into_iter()
        .filter_entry(|e| !e.path().to_str().unwrap().contains("target"))
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        // Chỉ xử lý file .rs
        if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            let mut noi_dung = String::new();
            File::open(path)?.read_to_string(&mut noi_dung)?;

            // Xóa comment dòng đơn
            let khong_comment = noi_dung
                .lines()
                .filter(|line| !line.trim().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");

            // Xóa comment khối
            let re = Regex::new(r"/\*[\s\S]*?\*/").unwrap();
            let khong_block_comment = re.replace_all(&khong_comment, "");

            // Nén khoảng trắng, bỏ dòng trống
            let nen = khong_block_comment
                .lines()
                .map(|line| line.trim())
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" ");

            // Thêm vào danh sách với đường dẫn tương đối
            let relative_path = path.strip_prefix(&current_dir).unwrap_or(path).to_string_lossy().into_owned();
            files_content.push(json!({
                "path": relative_path,
                "content": nen
            }));
        }
    }

    // Tạo JSON object
    let json_output = json!({
        "files": files_content
    });

    // Tạo thư mục cha nếu chưa tồn tại
    if let Some(parent) = std::path::Path::new(output_file).parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Ghi vào file JSON
    let mut file = File::create(output_file)?;
    file.write_all(serde_json::to_string_pretty(&json_output)?.as_bytes())?;
    println!("Đã tạo file JSON tại {}", output_file);
    Ok(())
}